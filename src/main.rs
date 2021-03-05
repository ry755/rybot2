use std::{sync::Arc, env, process::Command, convert::TryInto, num::NonZeroU16, io::Write, fs};
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
        gateway::Ready,
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
    rom: [u8; 512],
}

fn vec_to_array<T>(v: Vec<T>) -> [T; 512] {
    v.try_into()
        .unwrap_or_else(|v: Vec<T>| panic!("Expected a Vec of length {} but it was {}", 512, v.len()))
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
#[commands(about, say, boop, invert, ping, join, leave, play, dm, z80, z80asm)]
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

// joins the voice channel that the requesting user is currently in
#[command]
#[only_in(guilds)]
async fn join(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
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

    let manager = songbird::get(ctx).await
        .expect("Songbird Voice client placed in at initialization.").clone();

    let _handler = manager.join(guild_id, connect_to).await;

    Ok(())
}

// leaves the current voice channel
#[command]
#[only_in(guilds)]
async fn leave(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx).await
        .expect("Songbird Voice client placed in at initialization.").clone();
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

    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx).await
        .expect("Songbird Voice client placed in at initialization.").clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;

        let source = match songbird::ytdl(&url).await {
            Ok(source) => source,
            Err(why) => {
                println!("Error starting source: {:?}", why);

                send_msg(&ctx, &msg, "Error sourcing ffmpeg").await;

                return Ok(());
            },
        };

        handler.play_source(source);

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
        .output()?;

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

// execute Z80 opcodes and print the resulting register contents
#[command]
async fn z80(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let input = args.rest().split_whitespace().collect::<String>();
    let memory_vec = hex::decode(input).unwrap();

    z80_execute(&ctx, &msg, memory_vec).await
}

// assemble and execute Z80 assembly code and print the resulting register contents
#[command]
async fn z80asm(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let input = args.rest();

    let mut asm_file = Builder::new().suffix(".s").tempfile()?;
    println!("temp file location: {:?}", asm_file.path());

    asm_file.write(input.as_bytes()).unwrap();

    let vasm = Command::new("~/vasm/vasmz80_oldstyle")
    .arg("-dotdir")
    .arg("-Fbin")
    .arg(asm_file.path())
    .output().unwrap();

    let mut output_string = String::from("```");
    let closing_string = String::from("```");
    let vasm_stdout = String::from_utf8_lossy(&vasm.stdout);
    let vasm_stderr = String::from_utf8_lossy(&vasm.stderr);
    if vasm_stdout != "" {
        let vasm_stdout_clean = vasm_stdout.replace("```","`​`​`"); // add zero width spaces
        output_string.push_str(&vasm_stdout_clean);
        output_string.push_str(&closing_string);
    } else if vasm_stderr != "" {
        let vasm_stderr_clean = vasm_stdout.replace("```","`​`​`"); // add zero width spaces
        output_string.push_str(&vasm_stderr_clean);
        output_string.push_str(&closing_string);
    } else {
        output_string = String::from("Command completed with no output on `stdout` or `stderr`");
    }

    send_msg(&ctx, &msg, &output_string).await;

    let memory_vec = fs::read("a.out").expect("Unable to read file");
    z80_execute(&ctx, &msg, memory_vec).await
}

async fn z80_execute(ctx: &Context, msg: &Message, mut memory_vec: Vec<u8>) -> CommandResult {
    let mut tsc = TsClock::default();
    let mut cpu = Z80CMOS::default();

    // fill the remaining memory with halt opcodes
    for _ in 0..512-memory_vec.len() {
        memory_vec.push(0x76);
    }

    let mut memory = Bus { vdp: TMS9918A::new(), vdp_used: false, rom: vec_to_array(memory_vec) };

    let mut disassembly_string = String::from("Disassembly:\n```");

    cpu.reset();
    loop {
        match cpu.execute_next(&mut memory, &mut tsc,
                Some(|debug| disassembly_string.push_str(&format!("{}\n", format_args!("{:#X}", debug))) )) {
            Err(BreakCause::Halt) => { break }
            _ => {}
        }
    }

    memory.vdp.update();

    disassembly_string.push_str("```");

    let reg_a = cpu.get_reg(Reg8::A, None);
    let reg_bc = cpu.get_reg16(StkReg16::BC);
    let reg_de = cpu.get_reg16(StkReg16::DE);
    let reg_hl = cpu.get_reg16(StkReg16::HL);

    let mut reg_string = String::from("Z80 register contents on halt:\n");

    reg_string.push_str(&format!("`A:  {:#04X}`\n", reg_a));
    reg_string.push_str(&format!("`BC: {:#06X}`\n", reg_bc));
    reg_string.push_str(&format!("`DE: {:#06X}`\n", reg_de));
    reg_string.push_str(&format!("`HL: {:#06X}`\n", reg_hl));

    //send_msg(&ctx, &msg, &disassembly_string).await;
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
async fn about(ctx: &Context, msg: &Message) -> CommandResult {
    send_msg(&ctx, &msg, "This is rybot2! UwU").await;
    Ok(())
}

#[command]
async fn ping(ctx: &Context, msg: &Message) -> CommandResult {
    send_msg(&ctx, &msg, "Pong!").await;
    Ok(())
}