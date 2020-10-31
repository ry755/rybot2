use std::{env, sync::Arc, process::Command};
use serenity::{
    async_trait,
    client::bridge::gateway::ShardManager,
    client::bridge::voice::ClientVoiceManager,
    client::Context,
    framework::standard::{
        Args, CommandResult,
        Delimiter, StandardFramework,
        macros::{command, group, hook},
    },
    model::{
        channel::{Message, ReactionType},
        gateway::Ready,
        misc::Mentionable,
    },
    utils::{content_safe, ContentSafeOptions},
    voice,
};

use serenity::prelude::*;
use tokio::sync::Mutex;

use unicode_skeleton::UnicodeSkeleton;

use error_chain::error_chain;
use tempfile::Builder;

error_chain! {
    foreign_links {
        Io(std::io::Error);
        HttpRequest(reqwest::Error);
    }
}

extern crate image;

use image::{RgbImage, imageops};
use libwebp::WebPDecodeRGB;

// a container type is created for inserting into the Client's `data`, which
// allows for data to be accessible across all events and framework commands, or
// anywhere else that has a copy of the `data` Arc
struct ShardManagerContainer;

impl TypeMapKey for ShardManagerContainer {
    type Value = Arc<Mutex<ShardManager>>;
}

struct VoiceManager;

impl TypeMapKey for VoiceManager {
    type Value = Arc<Mutex<ClientVoiceManager>>;
}

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
}

#[group]
#[commands(about, say, boop, invert, shell, ping, join, leave, play)]
struct General;

#[hook]
async fn normal_message(ctx: &Context, msg: &Message) {
    let message_string_no_whitespace = msg.content.to_lowercase().split_whitespace().collect::<String>();
    let message_string = message_string_no_whitespace.skeleton_chars().collect::<String>();
    if message_string.contains("fox") {
        println!("{} found a fox OwO", msg.author.name);
        react_msg(ctx, msg, ReactionType::Unicode("🦊".to_string())).await;
    }
    if message_string.contains("cat") {
        println!("{} found a stinky cat :(", msg.author.name);
        react_msg(ctx, msg, ReactionType::Unicode("🐱".to_string())).await;
    }
    if message_string.contains("lemon") {
        println!("{} found a sour lemon", msg.author.name);
        react_msg(ctx, msg, ReactionType::Unicode("🍋".to_string())).await;
    }
}

async fn send_msg(ctx: &Context, msg: &Message, content: &str) {
    if let Err(why) = msg.channel_id.say(&ctx.http, &content).await {
        println!("Error sending message: {:?}", why);
    }
}

async fn react_msg(ctx: &Context, msg: &Message, reaction: ReactionType) {
    if let Err(why) = msg.react(&ctx.http, reaction).await {
        println!("Error reacting to message: {:?}", why);
    }
}

#[tokio::main]
async fn main() {
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    let framework = StandardFramework::new()
        .configure(|c| c
                   .with_whitespace(true)
                   .prefix("~"))
        .normal_message(normal_message)
        .group(&GENERAL_GROUP);

    let mut client = Client::builder(&token)
        .event_handler(Handler)
        .framework(framework)
        .await
        .expect("Error creating client");

    {
        let mut data = client.data.write().await;
        data.insert::<ShardManagerContainer>(Arc::clone(&client.shard_manager));
        data.insert::<VoiceManager>(Arc::clone(&client.voice_manager));
    }

    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
}

// joins the voice channel that the requesting user is currently in
#[command]
async fn join(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = match msg.guild(&ctx.cache).await {
        Some(guild) => guild,
        None => {
            send_msg(&ctx, &msg, "DMs not supported").await;

            return Ok(());
        }
    };

    let guild_id = guild.id;

    let channel_id = guild
        .voice_states.get(&msg.author.id)
        .and_then(|voice_state| voice_state.channel_id);

    let connect_to = match channel_id {
        Some(channel) => channel,
        None => {
            send_msg(&ctx, &msg, "Not in a voice channel").await;

            return Ok(());
        }
    };

    let manager_lock = ctx.data.read().await.get::<VoiceManager>().cloned().expect("Expected VoiceManager in TypeMap");
    let mut manager = manager_lock.lock().await;

    if manager.join(guild_id, connect_to).is_some() {
        send_msg(&ctx, &msg, &format!("Joined {}", connect_to.mention())).await;
    } else {
        send_msg(&ctx, &msg, "Error joining the channel").await;
    }

    Ok(())
}

// leaves the current voice channel
#[command]
async fn leave(ctx: &Context, msg: &Message) -> CommandResult {
    let guild_id = match ctx.cache.guild_channel_field(msg.channel_id, |channel| channel.guild_id).await {
        Some(id) => id,
        None => {
            send_msg(&ctx, &msg, "DMs not supported").await;

            return Ok(());
        },
    };

    let manager_lock = ctx.data.read().await.get::<VoiceManager>().cloned().expect("Expected VoiceManager in TypeMap");
    let mut manager = manager_lock.lock().await;
    let has_handler = manager.get(guild_id).is_some();

    if has_handler {
        manager.remove(guild_id);

        send_msg(&ctx, &msg, "Left voice channel").await;
    } else {
        send_msg(&ctx, &msg, "Not in a voice channel").await;
    }

    Ok(())
}


// plays audio from requested URL in the current voice channel
#[command]
async fn play(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let url = match args.single::<String>() {
        Ok(url) => url,
        Err(_) => {
            send_msg(&ctx, &msg, "Must provide a URL to a video or audio").await;

            return Ok(());
        },
    };

    if !url.starts_with("http") {
        send_msg(&ctx, &msg, "Must provide a valid URL").await;

        return Ok(());
    }

    let guild_id = match ctx.cache.guild_channel(msg.channel_id).await {
        Some(channel) => channel.guild_id,
        None => {
            send_msg(&ctx, &msg, "Error finding channel info").await;

            return Ok(());
        },
    };

    let manager_lock = ctx.data.read().await
        .get::<VoiceManager>().cloned().expect("Expected VoiceManager in TypeMap");
    let mut manager = manager_lock.lock().await;

    if let Some(handler) = manager.get_mut(guild_id) {
        let source = match voice::ytdl(&url).await {
            Ok(source) => source,
            Err(why) => {
                println!("Error starting source: {:?}", why);

                send_msg(&ctx, &msg, "Error sourcing ffmpeg").await;

                return Ok(());
            },
        };

        handler.play(source);

        send_msg(&ctx, &msg, "Playing song").await;
    } else {
        send_msg(&ctx, &msg, "Not in a voice channel to play in").await;
    }

    Ok(())
}

// repeats what the user passed as an argument
// user and role mentions are replaced with a safe textual alternative
#[command]
async fn say(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let settings = if let Some(guild_id) = msg.guild_id {
       ContentSafeOptions::default()
            .clean_channel(false)
            .display_as_member_from(guild_id)
    } else {
        ContentSafeOptions::default()
            .clean_channel(false)
            .clean_role(false)
    };

    let content = content_safe(&ctx.cache, &args.rest(), &settings).await;

    send_msg(&ctx, &msg, &content).await;

    Ok(())
}

// boops a user uwu
#[command]
async fn boop(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let mut parsed_args = Args::new(args.rest(), &[Delimiter::Single(' ')]);
    let mut boop_receiver = match parsed_args.single::<String>() {
        Ok(boop_receiver) => boop_receiver,
        Err(_) => {
            send_msg(&ctx, &msg, "Mention someone to boop! uwu").await;

            return Ok(());
        },
    };

    if &boop_receiver == "@everyone" {
        boop_receiver = "everyone".to_string();
    }

    let mut output = String::from("*");
    output.push_str(&msg.author.name);
    output.push_str(" boops ");
    output.push_str(&boop_receiver);
    output.push_str(" UwU*");

    send_msg(&ctx, &msg, &output).await;

    Ok(())
}

// inverts a user's profile picture
#[command]
async fn invert(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let mentioned_users = &msg.mentions;
    let user = &mentioned_users[0];
    let pfp_url = match user.avatar_url() {
        Some(pfp_url) => pfp_url,
        None => {
            send_msg(&ctx, &msg, "Failed to get URL for user").await;

            return Ok(());
        },
    };

    let response = reqwest::get(&pfp_url).await.expect("Bad response");
    let content = response.bytes().await.expect("Failed to get response");

    let file = Builder::new().suffix(".png").tempfile().expect("Failed to create temp file");

    let (width, height, buf) = WebPDecodeRGB(content.as_ref()).expect("Invalid WebP header");
    let mut pixel_buf = match RgbImage::from_vec(width, height, buf.to_vec()) {
        Some(pixel_buf) => pixel_buf,
        None => return Ok(()) // return from command early
    };

    imageops::invert(&mut pixel_buf);

    let file_path = match file.path().to_str() {
        Some(file_path) => file_path,
        None => return Ok(()) // return from command early
    };
    println!("temp file location: {:?}", file_path);
    pixel_buf.save(file_path).expect("Failed to save file");
    let path = vec![file_path];
    if let Err(why) = msg.channel_id.send_files(&ctx.http, path, |m| {
        m.content("")
    }).await {
        println!("Error sending file: {:?}", why);
    }

    Ok(())
}

// execute a bash command and send the output
#[command]
async fn shell(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let settings = if let Some(guild_id) = msg.guild_id {
        ContentSafeOptions::default()
            .clean_channel(false)
            .display_as_member_from(guild_id)
    } else {
        ContentSafeOptions::default()
            .clean_channel(false)
            .clean_role(false)
    };

    let bash_args = content_safe(&ctx.cache, &args.rest(), &settings).await;
    println!("{}", bash_args);
    //let bash_args = args.rest();
    let bash_shell = Command::new("bash")
        .arg("-c")
        .arg(bash_args)
        .output().expect("bruh");

    let mut output_string = String::from("```");
    let closing_string = String::from("```");
    let bash_stdout = String::from_utf8_lossy(&bash_shell.stdout);
    let bash_stderr = String::from_utf8_lossy(&bash_shell.stderr);
    if bash_stdout != "" {
        let bash_stdout_clean = bash_stdout.replace("```","`​`​`"); // add zero width spaces
        output_string.push_str(&bash_stdout_clean);
        output_string.push_str(&closing_string);
    } else if bash_stderr != "" {
        let bash_stderr_clean = bash_stdout.replace("```","`​`​`"); // add zero width spaces
        output_string.push_str(&bash_stderr_clean);
        output_string.push_str(&closing_string);
    } else {
        output_string = String::from("Command completed with no output on `stdout` or `stderr`");
    }

    send_msg(&ctx, &msg, &output_string).await;

    Ok(())
}

#[command]
async fn about(ctx: &Context, msg: &Message) -> CommandResult {
    send_msg(&ctx, &msg, "This is rybot2! UwU").await;
    Ok(())
}

#[command]
async fn ping(ctx: &Context, msg: &Message) -> CommandResult {
    send_msg(&ctx, &msg, "Pong!").await;
    Ok(())
}