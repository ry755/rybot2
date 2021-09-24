use error_chain::error_chain;
use libwebp::WebPDecodeRGB;
use std::{env, sync::Arc};
use serenity::{
    async_trait,
    client::Context,
    client::bridge::gateway::ShardManager,
    framework::standard::{
        Args, CommandResult,
        Delimiter, StandardFramework,
        macros::{command, group, hook},
    },
    model::{
        channel::{Message, ReactionType},
        gateway::{Activity, Ready},
    },
    utils::{content_safe, ContentSafeOptions},
    prelude::*,
};
use image::{RgbImage, imageops};
use songbird::SerenityInit;
use tempfile::Builder;

struct ShardManagerContainer;
impl TypeMapKey for ShardManagerContainer {
    type Value = Arc<Mutex<ShardManager>>;
}

error_chain! {
    foreign_links {
        Io(std::io::Error);
        HttpRequest(reqwest::Error);
    }
}

struct Handler;
#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
}

#[group]
#[commands(help, activity, say, boop, dm, pfp, invert, ping, join, leave, play, skip, stop, np)]
struct General;

#[hook]
async fn normal_message(ctx: &Context, msg: &Message) {
    let message_string = msg.content.to_lowercase().split_whitespace().collect::<String>();
    if message_string.contains("fox") {
        //println!("{} found a fox OwO", msg.author.name);
        react_msg(ctx, msg, ReactionType::Unicode("🦊".to_string())).await;
    }
    if message_string.contains("cat") {
        //println!("{} found a stinky cat :(", msg.author.name);
        react_msg(ctx, msg, ReactionType::Unicode("🐱".to_string())).await;
    }
    if message_string.contains("lemon") {
        //println!("{} found a sour lemon", msg.author.name);
        react_msg(ctx, msg, ReactionType::Unicode("🍋".to_string())).await;
    }
}

async fn send_msg(ctx: &Context, msg: &Message, content: &str) {
    if let Err(reason) = msg.channel_id.say(&ctx.http, &content).await {
        println!("Error sending message: {:?}", reason);
    }
}

async fn react_msg(ctx: &Context, msg: &Message, reaction: ReactionType) {
    if let Err(reason) = msg.react(&ctx.http, reaction).await {
        println!("Error reacting to message: {:?}", reason);
    }
}


async fn send_file(ctx: &Context, msg: &Message, path: Vec<&str>) {
    if let Err(reason) = msg.channel_id.send_files(&ctx.http, path, |m| {
        m.content("")
    }).await {
        println!("Error sending file: {:?}", reason);
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

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
        .register_songbird()
        .await
        .expect("Error creating client");

    {
        let mut data = client.data.write().await;
        data.insert::<ShardManagerContainer>(Arc::clone(&client.shard_manager));
    }

    if let Err(reason) = client.start().await {
        println!("Client error: {:?}", reason);
    }
}

#[command]
async fn help(ctx: &Context, msg: &Message) -> CommandResult {
    let mut help_string = format!("rybot2 {} ({})\n", env!("VERGEN_BUILD_SEMVER"), env!("VERGEN_GIT_SHA_SHORT"));
    help_string.push_str(&format!("compiled on {} at {} ({})\n", env!("VERGEN_BUILD_DATE"), env!("VERGEN_BUILD_TIME"), env!("VERGEN_CARGO_PROFILE")));
    help_string.push_str(&format!("rustc {} ({})\n\n", env!("VERGEN_RUSTC_SEMVER"), env!("VERGEN_RUSTC_HOST_TRIPLE")));

    let audio_command_help_string = "audio playback commands:
    `join`: join the current voice channel
    `leave`: leave the current voice channel
    `play`: queue/play the specified URL, or search YouTube and queue/play the first result
    `skip`: skip the currently playing audio in the queue
    `stop`: clear the audio queue
    `np`: view current audio playback info\n\n";
    let misc_command_help_string = "misc commands:
    `help`: list valid commands and some system info
    `say`: print a message
    `boop`: boop another user :3
    `dm`: send a DM to a user
    `pfp`: send the profile picture of a user (defaults to yourself if no username is mentioned)
    `invert`: send the profile picture of a user with inverted colors (defaults to yourself if no username is mentioned)";
    help_string.push_str(audio_command_help_string);
    help_string.push_str(misc_command_help_string);

    send_msg(&ctx, &msg, &help_string).await;
    Ok(())
}

// sets the activity specified by the user
#[command]
async fn activity(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let activity = args.rest();
    ctx.set_activity(Activity::playing(activity)).await;
    send_msg(&ctx, &msg, &format!("Activity set to \"Playing {}\"", activity)).await;
    Ok(())
}

async fn join_impl(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let channel_id = guild.voice_states.get(&msg.author.id).and_then(|voice_state| voice_state.channel_id);

    let connect_to = match channel_id {
        Some(channel) => channel,
        None => {
            send_msg(&ctx, &msg, "Not in a voice channel").await;

            return Ok(());
        }
    };

    let manager = songbird::get(ctx).await.expect("Error getting Songbird client").clone();

    let _handler = manager.join(guild_id, connect_to).await;
    send_msg(&ctx, &msg, "Joined voice channel").await;

    Ok(())
}

// joins the voice channel that the requesting user is currently in
#[command]
#[only_in(guilds)]
async fn join(ctx: &Context, msg: &Message) -> CommandResult {
    join_impl(ctx, msg).await
}

// leaves the current voice channel
#[command]
#[only_in(guilds)]
async fn leave(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx).await.expect("Error getting Songbird client").clone();
    let has_handler = manager.get(guild_id).is_some();

    if has_handler {
        if let Err(reason) = manager.remove(guild_id).await {
            send_msg(&ctx, &msg, format!("Failed: {:?}", reason).as_str()).await;
        }

        send_msg(&ctx, &msg, "Left voice channel").await;
    } else {
        send_msg(&ctx, &msg, "Not in a voice channel").await;
    }

    Ok(())
}


// plays audio from requested URL in the current voice channel
#[command]
#[only_in(guilds)]
async fn play(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let url_or_search = args.rest();
    let mut should_search = false;
    if !url_or_search.starts_with("http") {
        //send_msg(&ctx, &msg, "Must provide a valid URL").await;
        //return Ok(());
        should_search = true;
    }

    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx).await.expect("Error getting Songbird client").clone();

    let handler_option = manager.get(guild_id);
    if let None = handler_option {
        if let Err(_) = join_impl(ctx, msg).await {}
    }

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;

        let source =
            if should_search {
                songbird::input::ytdl_search(&url_or_search).await
            } else {
                songbird::input::ytdl(&url_or_search).await
            };

        let source = match source {
            Ok(source) => source,
            Err(reason) => {
                println!("Error starting source: {:?}", reason);
                send_msg(&ctx, &msg, &format!("Error starting source: {:?}", reason)).await;
                return Ok(());
            },
        };

        {
            let source_url_option = (&source.metadata.source_url).clone();
            let source_url = source_url_option.unwrap_or("Unable to extract source URL".to_string());
            let queue_or_play = if handler.queue().is_empty() { "Playing" } else { "Queuing" };
            send_msg(&ctx, &msg, &format!("{} audio ({})", queue_or_play, source_url)).await;
        }

        handler.enqueue_source(source);
    } else {
        send_msg(&ctx, &msg, "Not in a voice channel").await;
    }

    Ok(())
}

// skips currently playing audio
#[command]
#[only_in(guilds)]
async fn skip(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx).await.expect("Error getting Songbird client").clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;

        match handler.queue().skip() {
            Ok(_) => {},
            Err(reason) => send_msg(&ctx, &msg, &format!("Error skipping audio: {:?}", reason)).await,
        }

        send_msg(&ctx, &msg, "Skipped audio").await;
    } else {
        send_msg(&ctx, &msg, "Not in a voice channel").await;
    }

    Ok(())
}

// stops all audio playback
#[command]
#[only_in(guilds)]
async fn stop(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx).await.expect("Error getting Songbird client").clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;

        handler.queue().stop();

        send_msg(&ctx, &msg, "Stopped audio playback").await;
    } else {
        send_msg(&ctx, &msg, "Not in a voice channel").await;
    }

    Ok(())
}

// sends current audio playback info
#[command]
#[only_in(guilds)]
async fn np(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx).await.expect("Error getting Songbird client").clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;

        let current_track_or_error = handler.queue().current();
        let current_track = match current_track_or_error {
            Some(current_track) => current_track,
            None => {
                send_msg(&ctx, &msg, "No audio track appears to be playing at the moment").await;
                return Ok(());
            }
        };
        let song_title = current_track.metadata().title.clone();
        let song_track = current_track.metadata().track.clone();
        let song_artist = current_track.metadata().artist.clone();
        let song_yt_channel = current_track.metadata().channel.clone();
        let song_url = current_track.metadata().source_url.clone();

        let mut song_string = "Currently playing audio track:\n".to_string();
        song_string.push_str(&format!("    title: {}\n", song_title.unwrap_or("none".to_string())));
        song_string.push_str(&format!("    track: {}\n", song_track.unwrap_or("none".to_string())));
        song_string.push_str(&format!("    artist: {}\n", song_artist.unwrap_or("none".to_string())));
        song_string.push_str(&format!("    YouTube channel: {}\n", song_yt_channel.unwrap_or("none".to_string())));
        song_string.push_str(&format!("    URL: <{}>", song_url.unwrap_or("none".to_string())));
        send_msg(&ctx, &msg, &song_string).await;
    } else {
        send_msg(&ctx, &msg, "Not in a voice channel").await;
    }

    Ok(())
}

// repeats what the user passed as an argument
// user and role mentions are replaced with a safe textual alternative
#[command]
async fn say(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let settings = if let Some(guild_id) = msg.guild_id {
        ContentSafeOptions::default().clean_channel(false).display_as_member_from(guild_id)
    } else {
        ContentSafeOptions::default().clean_channel(false).clean_role(false)
    };

    let content = content_safe(&ctx.cache, &args.rest(), &settings).await;

    send_msg(&ctx, &msg, &content).await;

    Ok(())
}

// sends a DM to the specified user
#[command]
async fn dm(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let user = &msg.mentions.get(0);
    match user {
        Some(user) => user,
        None => {
            send_msg(&ctx, &msg, "Mention someone to DM! uwu").await;

            return Ok(()); // return from command early
        },
    };
    // this is probably a bad way of removing the mentioned user from the argument string
    let mut parsed_args = Args::new(args.rest(), &[Delimiter::Single(' ')]);
    parsed_args.advance();
    let mut message = msg.author.name.to_string();
    message.push_str(" says ");
    message.push_str(parsed_args.rest());

    // using unwrap() should be ok here since we already know we have a good value
    let _ = user.unwrap().dm(&ctx.http, |m| {
        m.content(message);
        m
    }).await;

    send_msg(&ctx, &msg, "Message sent! :3").await;

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
    output.push_str("* :3");

    send_msg(&ctx, &msg, &output).await;

    Ok(())
}

// sends a user's profile picture
#[command]
async fn pfp(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let user = &msg.mentions.get(0).unwrap_or(&msg.author);
    let pfp_url = match user.avatar_url() {
        Some(pfp_url) => pfp_url,
        None => {
            send_msg(&ctx, &msg, "Failed to get URL for user").await;
            return Ok(());
        },
    };

    send_msg(&ctx, &msg, &pfp_url).await;

    Ok(())
}

// inverts a user's profile picture
#[command]
async fn invert(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let user = &msg.mentions.get(0).unwrap_or(&msg.author);
    let pfp_url = match user.avatar_url() {
        Some(pfp_url) => pfp_url,
        None => {
            send_msg(&ctx, &msg, "Failed to get URL for user").await;
            return Ok(());
        },
    };

    let response = reqwest::get(&pfp_url).await?;
    let content = response.bytes().await?;

    let file = Builder::new().suffix(".png").tempfile()?;

    let (width, height, buf) = WebPDecodeRGB(content.as_ref())?;
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
    pixel_buf.save(file_path)?;
    let path = vec![file_path];
    send_file(&ctx, &msg, path).await;

    Ok(())
}

#[command]
async fn ping(ctx: &Context, msg: &Message) -> CommandResult {
    send_msg(&ctx, &msg, "Pong!").await;
    Ok(())
}