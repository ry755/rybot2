use std::{convert::TryInto, env, num::NonZeroU16, sync::Arc};
//use std::process::Command;
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

use songbird::SerenityInit;

use tms9918a_emu::TMS9918A;

struct ShardManagerContainer;

impl TypeMapKey for ShardManagerContainer {
    type Value = Arc<Mutex<ShardManager>>;
}

use z80emu::*;
extern crate hex;
type TsClock = host::TsCounter<i32>;

// Z80 memory
struct Bus {
    vdp: TMS9918A,
    vdp_used: bool,
    rom: [u8; 65536],
}

fn vec_to_array<T>(v: Vec<T>) -> [T; 65536] {
    v.try_into()
        .unwrap_or_else(|v: Vec<T>| panic!("Expected a Vec of length {} but it was {}", 65536, v.len()))
}

impl Io for Bus {
    type Timestamp = i32;
    type WrIoBreak = ();
    type RetiBreak = ();

    #[inline(always)]
    fn write_io(&mut self, port: u16, data: u8, _ts: i32) -> (Option<()>, Option<NonZeroU16>) {
        let masked_port = port & 0x00FF;
        //println!("[write_io] masked_port: {:#X}, data: {:#X}", masked_port, data);
        // VDP control
        if masked_port == 0b00010010 {
            //println!("[write_io] writing to VDP control port");
            self.vdp_used = true;
            self.vdp.write_control_port(data);
        }

        // VDP data
        if masked_port == 0b00010000 {
            //println!("[write_io] writing to VDP data port");
            self.vdp_used = true;
            self.vdp.write_data_port(data);
        }

        (None, None)
    }

    #[inline(always)]
    fn read_io(&mut self, port: u16, _ts: i32) -> (u8, Option<NonZeroU16>) {
        let masked_port = port & 0x00FF;
        // VDP data
        if masked_port == 0b00010000 {
            //println!("[read_io ] reading from VDP data port");
            self.vdp_used = true;
            return (self.vdp.read_data_port(), None);
        }

        (0, None)
    }
}

impl Memory for Bus {
    type Timestamp = i32;
    fn read_debug(&self, addr: u16) -> u8 {
        self.rom[addr as usize]
    }

    fn write_mem(&mut self, addr: u16, value: u8, _ts: Self::Timestamp) {
        self.rom[addr as usize] = value;
    }
}

use error_chain::error_chain;
use tempfile::Builder;

error_chain! {
    foreign_links {
        Io(std::io::Error);
        HttpRequest(reqwest::Error);
    }
}

//extern crate image;

use image::{ImageBuffer, Rgb, RgbImage, imageops};
use libwebp::WebPDecodeRGB;

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
}

#[group]
//#[commands(about, say, boop, invert, shell, ping, join, leave, play, dm, z80)]
#[commands(help, activity, say, boop, pfp, invert, ping, join, leave, play, stop, dm, z80, z80file)]
struct General;

#[hook]
async fn normal_message(ctx: &Context, msg: &Message) {
    let message_string = msg.content.to_lowercase().split_whitespace().collect::<String>();
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


async fn send_file(ctx: &Context, msg: &Message, path: Vec<&str>) {
    if let Err(why) = msg.channel_id.send_files(&ctx.http, path, |m| {
        m.content("")
    }).await {
        println!("Error sending file: {:?}", why);
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

    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
}

#[command]
async fn help(ctx: &Context, msg: &Message) -> CommandResult {
    let mut help_string = format!("rybot2 {} ({})\n", env!("VERGEN_BUILD_SEMVER"), env!("VERGEN_GIT_SHA_SHORT"));
    help_string.push_str(&format!("compiled on {} at {} ({})\n", env!("VERGEN_BUILD_DATE"), env!("VERGEN_BUILD_TIME"), env!("VERGEN_CARGO_PROFILE")));
    help_string.push_str(&format!("rustc {} ({})\n\n", env!("VERGEN_RUSTC_SEMVER"), env!("VERGEN_RUSTC_HOST_TRIPLE")));

    let command_help_string = "commands:
    `help`: list valid commands and some system info
    `say`: print a message
    `boop`: boop another user :3
    `dm`: send a DM to a user
    `pfp`: send the profile picture of a user (defaults to yourself if no username is mentioned)
    `invert`: send the profile picture of a user with inverted colors (defaults to yourself if no username is mentioned)
    `join`: join the current voice channel
    `leave`: leave the current voice channel
    `play`: play the specified URL, or search YouTube and play the first result
    `stop`: stop any currently playing audio
    `z80`: execute the specified opcodes (in hexadecimal) in the Z80 emulator and print the register results on halt
    `z80file`: like `z80` except it loads the opcodes from the attached file";
    help_string.push_str(command_help_string);

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
        if let Err(e) = manager.remove(guild_id).await {
            send_msg(&ctx, &msg, format!("Failed: {:?}", e).as_str()).await;
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
            Err(why) => {
                println!("Error starting source: {:?}", why);
                send_msg(&ctx, &msg, &format!("Error starting source: {:?}", why)).await;
                return Ok(());
            },
        };

        {
            let source_url_option = (&source.metadata.source_url).clone();
            let source_url = source_url_option.unwrap_or("Unable to extract source URL".to_string());
            send_msg(&ctx, &msg, &format!("Playing audio ({})", source_url)).await;
        }

        handler.play_only_source(source);
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
        let mut handler = handler_lock.lock().await;

        handler.stop();

        send_msg(&ctx, &msg, "Stopped audio playback").await;
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

/*// execute a bash command and send the output
#[command]
async fn shell(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let settings = if let Some(guild_id) = msg.guild_id {
        ContentSafeOptions::default().clean_channel(false).display_as_member_from(guild_id)
    } else {
        ContentSafeOptions::default().clean_channel(false).clean_role(false)
    };

    let bash_args = content_safe(&ctx.cache, &args.rest(), &settings).await;
    println!("{}", bash_args);
    //let bash_args = args.rest();
    let bash_shell = Command::new("bash").arg("-c").arg(bash_args).output()?;

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
}*/

// execute Z80 opcodes and print the resulting register contents
#[command]
async fn z80(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let input = args.rest().split_whitespace().collect::<String>();
    let memory_vec = hex::decode(input).unwrap();

    z80_execute(&ctx, &msg, memory_vec).await
}

// execute Z80 opcodes from an uploaded file and print the resulting register contents
#[command]
async fn z80file(ctx: &Context, msg: &Message) -> CommandResult {
    let memory_vec = msg.attachments[0].download().await.unwrap();

    z80_execute(&ctx, &msg, memory_vec).await
}

async fn z80_execute(ctx: &Context, msg: &Message, mut memory_vec: Vec<u8>) -> CommandResult {
    let mut tsc = TsClock::default();
    let mut cpu = Z80CMOS::default();

    // fill the remaining memory with halt opcodes
    for _ in 0..65536-memory_vec.len() {
        memory_vec.push(0x76);
    }

    let mut memory = Bus { vdp: TMS9918A::new(), vdp_used: false, rom: vec_to_array(memory_vec) };

    let mut disassembly_string = String::from("Disassembly:\n```");

    let mut flag = false;
    let _ = disasm::disasm_memory::<Z80CMOS, _, ()>(0x0000, &mut memory.rom, 
        |debug| {
            disassembly_string.push_str(&format!("{}\n", format_args!("{:#X}", debug)));
            // if two halt instructions are found in a row, exit
            if debug.code.as_slice() == [0x76] {
                if flag == true {
                    return Err(());
                }
                flag = true;
            } else {
                flag = false;
            }
            Ok(())
        });

    disassembly_string.push_str("```");
    send_msg(&ctx, &msg, &disassembly_string).await;
    println!("{}", &disassembly_string);

    cpu.reset();
    loop {
        match cpu.execute_next(&mut memory, &mut tsc,
                Some(|_| print!("") )) {
            Err(BreakCause::Halt) => { break }
            _ => {}
        }
    }

    memory.vdp.update();

    let reg_a = cpu.get_reg(Reg8::A, None);
    let reg_bc = cpu.get_reg16(StkReg16::BC);
    let reg_de = cpu.get_reg16(StkReg16::DE);
    let reg_hl = cpu.get_reg16(StkReg16::HL);

    let mut reg_string = String::from("Z80 register contents on halt:\n");

    reg_string.push_str(&format!("`A:  {:#04X}`\n", reg_a));
    reg_string.push_str(&format!("`BC: {:#06X}`\n", reg_bc));
    reg_string.push_str(&format!("`DE: {:#06X}`\n", reg_de));
    reg_string.push_str(&format!("`HL: {:#06X}`\n", reg_hl));

    send_msg(&ctx, &msg, &reg_string).await;

    reg_string = String::from("TMS9918A register contents on halt:\n");

    reg_string.push_str(&format!("`0: {:#04X} | 4: {:#04X}`\n", memory.vdp.read_register(0), memory.vdp.read_register(4)));
    reg_string.push_str(&format!("`1: {:#04X} | 5: {:#04X}`\n", memory.vdp.read_register(1), memory.vdp.read_register(5)));
    reg_string.push_str(&format!("`2: {:#04X} | 6: {:#04X}`\n", memory.vdp.read_register(2), memory.vdp.read_register(6)));
    reg_string.push_str(&format!("`3: {:#04X} | 7: {:#04X}`\n", memory.vdp.read_register(3), memory.vdp.read_register(7)));

    if memory.vdp_used == false {
        Ok(())
    } else {
        send_msg(&ctx, &msg, &reg_string).await;

        let vdp_file = Builder::new().suffix(".png").tempfile()?;

        let width = memory.vdp.frame_width as u32;
        let height = memory.vdp.frame_height as u32;
        let mut pixel_buf = ImageBuffer::from_fn(width, height, |x, y| {
            Rgb([
                ((memory.vdp.frame[((y * width) + x) as usize] & 0xFF0000) >> 16) as u8,
                ((memory.vdp.frame[((y * width) + x) as usize] & 0x00FF00) >> 8) as u8,
                (memory.vdp.frame[((y * width) + x) as usize] & 0x0000FF) as u8,
            ])
        });

        pixel_buf = imageops::resize(&mut pixel_buf, width * 4, height * 4, imageops::FilterType::Nearest);

        let file_path = match vdp_file.path().to_str() {
            Some(file_path) => file_path,
            None => {
                send_msg(&ctx, &msg, "Failed to get file path of image buffer for emulated TMS9918A").await;
                return Ok(())
            }
        };
        send_msg(&ctx, &msg, "TMS9918A video output:").await;
        println!("temp file location: {:?}", file_path);
        pixel_buf.save(file_path).unwrap();
        let path = vec![file_path];
        send_file(&ctx, &msg, path).await;

        Ok(())
    }
}

#[command]
async fn ping(ctx: &Context, msg: &Message) -> CommandResult {
    send_msg(&ctx, &msg, "Pong!").await;
    Ok(())
}