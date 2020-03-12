use base64;
use env_logger;
use image;
use image::ImageOutputFormat::Png;
use lazy_static::lazy_static;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serenity::{
    framework::standard::{
        help_commands,
        macros::{command, group, help},
        Args, CommandError, CommandGroup, CommandResult, DispatchError, HelpOptions,
        StandardFramework,
    },
    http::Http,
    model::{
        channel::{Message, ReactionType},
        gateway::Ready,
        guild::Emoji,
        id::{ChannelId, GuildId, MessageId, UserId},
    },
    prelude::*,
};
use std::{
    collections::{HashMap, HashSet},
    env,
    ffi::OsStr,
    path::Path,
    sync::{Arc, RwLock},
};

#[derive(Serialize, Deserialize, Debug)]
struct User {
    name: String,
    counter: u64,
}

#[derive(Serialize, Deserialize, Debug)]
struct EmoteMessage {
    messages: [Message; 2],
    emote: Emote,
}

#[derive(Serialize, Deserialize, Debug)]
struct Emote {
    name: String,
    author: String,
}

const CHANNEL: ChannelId = ChannelId(292651939555049472);
const GUILD: GuildId = GuildId(292651939555049472);

lazy_static! {
    static ref USERS: RwLock<HashMap<UserId, User>> = RwLock::new(HashMap::new());
    static ref MESSAGES: RwLock<HashMap<MessageId, EmoteMessage>> = RwLock::new(HashMap::new());
}

struct Handler;

impl EventHandler for Handler {
    fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
}

#[group]
#[commands(add, stats, remove)]
struct General;

#[help]
#[individual_command_tip = "If you want more information about a specific command, just pass the command as argument."]
#[command_not_found_text = "Could not find: `{}`."]
#[max_levenshtein_distance(3)]
#[lacking_permissions = "Hide"]
fn my_help(
    context: &mut Context,
    msg: &Message,
    args: Args,
    help_options: &'static HelpOptions,
    groups: &[&'static CommandGroup],
    owners: HashSet<UserId>,
) -> CommandResult {
    help_commands::with_embeds(context, msg, args, help_options, groups, owners)
}

#[command]
#[only_in(guilds)]
#[example("FeelsGoodMan [image as attachment]")]
fn add(ctx: &mut Context, msg: &Message, mut args: Args) -> CommandResult {
    let http = ctx.http.clone();
    let mut users = USERS.write().unwrap();

    println!("{}   Args for stats: {}", msg.author.name, &args.message());
    let user = users.entry(msg.author.id).or_insert(User {
        name: msg.author.name.clone(),
        counter: 0,
    });

    if user.counter == 3 {
        return dm_user_err(http, msg, "You can only post 3 suggestions.");
    }

    // check for the name
    let name = match args.single::<String>() {
        Ok(x) => x,
        Err(_) => return dm_user_err(http, msg, "No name found."),
    };

    // check if there is exactly one attachment
    if msg.attachments.len() != 1 {
        return dm_user_err(http, msg, "No attachment found.");
    }

    let attachment = msg.attachments.first().unwrap();

    //check emoji size, max 6MB
    if attachment.size >= 6_000_000 {
        return dm_user_err(http, msg, "6MB is the size limit for images.");
    }

    // check if the attachment is an image and check for best size of emotes (128x128px)
    match attachment.dimensions() {
        Some(dimensions) => {
            if dimensions.0 < 120 || dimensions.1 < 120 {
                return dm_user_err(http, msg, "Image must be at least 128x128px.");
            }
        }
        None => return dm_user_err(http, msg, "Attachment is not an image."),
    };

    // get the attachment
    let img = match attachment.download() {
        Ok(x) => x,
        Err(why) => {
            dm_user(http, msg, "Attachment download failed, try again later.");
            return Err(CommandError(format!("Attachment download: {:?}", why)));
        }
    };

    // delete original message after download is finished!
    match msg.delete(http.clone()) {
        Ok(_) => {}
        Err(why) => {
            dm_user(http, msg, "Discord error, pls try again later.");
            return Err(CommandError(format!("Deleting org. msg: {:?}", why)));
        }
    }

    // split the filename with extension
    let filetype = match Path::new(&attachment.filename)
        .extension()
        .and_then(OsStr::to_str)
    {
        Some(x) => x,
        None => return dm_user_err(http, msg, "Filename is not processable."),
    };

    // check image type
    if !(vec!["jpeg", "jpg", "png"].contains(&filetype)) {
        return dm_user_err(http, msg, "JPG, JPEG or PNG, nothing else is allowed.");
    }

    let mut buf = Vec::new();
    let emote = Emote {
        name: name.clone(),
        author: msg.author.name.to_string(),
    };

    let img = match image::load_from_memory(&img) {
        Ok(img) => img,
        Err(why) => {
            dm_user(http, msg, "Error processing image.");
            return Err(CommandError(format!("Processing image: {:?}", why)));
        }
    };
    img.thumbnail_exact(128, 128).write_to(&mut buf, Png)?;
    let emote_string = base64::encode(&buf);

    let em: Emoji = match GUILD.create_emoji(
        http.clone(),
        &*emote.name,
        &*format!("data:image/png;base64,{}", emote_string),
    ) {
        Ok(x) => x,
        Err(why) => {
            dm_user(http, msg, "Discord error, pls try again later.");
            return Err(CommandError(format!("Creating emote: {:?}", why)));
        }
    };

    let bot_msg1 = match CHANNEL.send_message(&ctx.http, |m| {
        m.content(format!("{}", emote.name));
        m.add_files(vec![(&*buf, &*format!("{}.png", name))])
    }) {
        Ok(x) => x,
        Err(why) => {
            dm_user(http, msg, "Discord error, pls try again later.");
            return Err(CommandError(format!("Sending msg one: {:?}", why)));
        }
    };

    let bot_msg2 = match CHANNEL.send_message(&ctx.http, |m| {
        m.content(format!("<:{}:{}>", em.name, em.id));
        m.reactions(vec![ReactionType::from("👍"), ReactionType::from("👎")])
    }) {
        Ok(x) => x,
        Err(why) => {
            dm_user(http, msg, "Discord error, pls try again later.");
            return Err(CommandError(format!("Sending msg one: {:?}", why)));
        }
    };

    MESSAGES.write().unwrap().insert(
        bot_msg2.id.clone(),
        EmoteMessage {
            messages: [bot_msg1, bot_msg2],
            emote,
        },
    );
    user.counter += 1;

    if let Err(why) = em.delete(ctx) {
        dm_user(http, msg, "Internal error, pls DM Infi#8527.");
        return Err(CommandError(format!("Deleting emote: {:?}", why)));
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
#[allowed_roles("Moderator", "admin")]
fn stats(ctx: &mut Context, msg: &Message) -> CommandResult {
    let http = ctx.http.clone();
    let messages = MESSAGES.read().unwrap();

    let content: String = messages
        .values()
        .collect::<Vec<_>>()
        .into_par_iter()
        .filter_map(|emsg: &EmoteMessage| {
            emsg.messages[1]
                .channel_id
                .message(&http, emsg.messages[1].id)
                .ok()
                .map(|m| (emsg, m))
        })
        .map(|(emsg, umsg)| {
            let (pos, neg) =
                umsg.reactions
                    .iter()
                    .fold((0, 0), |(pos, neg), r| match &r.reaction_type {
                        ReactionType::Unicode(n) if n == "👍" => (r.count, neg),
                        ReactionType::Unicode(n) if n == "👎" => (pos, r.count),
                        _ => (pos, neg),
                    });
            if pos * neg == 0 {
                return String::from("Error, could not retrieve votes");
            }
            format!(
                "\n{}: {:.6} from: {}",
                emsg.emote.name,
                pos as f64 / neg as f64,
                emsg.emote.author
            )
        })
        .reduce(String::new, |acc, s| acc + &s);

    if let Err(why) = msg.channel_id.say(ctx, &content) {
        dm_user(http, msg, "Discord error, pls try again later.");
        return Err(CommandError(format!(
            "Sending msg: {:?}, message was: {}",
            why, content
        )));
    };

    Ok(())
}

#[command]
#[only_in(guilds)]
#[example("123456789")]
#[allowed_roles("Moderator", "admin")]
fn remove(ctx: &mut Context, msg: &Message, mut args: Args) -> CommandResult {
    let http = ctx.http.clone();
    let mut messages = MESSAGES.write().unwrap();

    println!("{}   Args for stats: {}", msg.author.name, &args.message());
    let parsed = args
        .single::<u64>()
        .map(|id| MessageId(id))
        .map_err(|_| "Missing id.")
        .and_then(|id| {
            messages
                .get(&id)
                .map(|m| (id, m))
                .ok_or("ID is not in messages.")
        })
        .and_then(|(id, m)| {
            match m
                .messages
                .iter()
                .map(|m| m.delete(http.clone()))
                .all(|r| r.is_ok())
            {
                true => (Ok(id)),
                false => Err("Internal error, pls try again later."),
            }
        });

    match parsed {
        Ok(id) => messages.remove(&id),
        Err(mess) => return dm_user_err(http, msg, mess),
    };

    dm_user(http, msg, "Done");
    Ok(())
}

pub fn send(http: Arc<Http>, target: ChannelId, content: &str) {
    if let Err(why) = target.say(http, content) {
        println!("Could not send message: {:?}", why);
    }
}

fn dm_user(http: Arc<Http>, msg: &Message, content: &str) {
    if let Err(why) = msg.author.dm(http.clone(), |m| m.content(content)) {
        println!("Could not send message to {}: {:?}", msg.author, why);
        send(http, msg.channel_id, content)
    }
}

fn dm_user_err(http: Arc<Http>, msg: &Message, content: &str) -> CommandResult {
    if let Err(why) = msg.author.dm(http.clone(), |m| m.content(&content)) {
        println!("Could not send message to {}: {:?}", msg.author, why);
        send(http, msg.channel_id, content)
    }

    return Err(CommandError(content.to_string()));
}

fn main() {
    env_logger::init();

    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");
    let mut client = Client::new(&token, Handler).expect("Err creating client");

    client.with_framework(
        StandardFramework::new()
            .configure(|c| c.with_whitespace(true).prefix(">>").delimiters(vec![" "]))
            .after(|_, _, command_name, error| match error {
                Ok(()) => {}
                Err(why) => println!("Command {} returned error {:?}", command_name, why),
            })
            .on_dispatch_error(|ctx, msg, error| {
                if let DispatchError::Ratelimited(seconds) = error {
                    let _ = msg.channel_id.say(
                        &ctx.http,
                        &format!("Try this again in {} seconds.", seconds),
                    );
                }
            })
            .help(&MY_HELP)
            .group(&GENERAL_GROUP),
    );

    if let Err(why) = client.start() {
        println!("Client error: {:?}", why);
    }
}
