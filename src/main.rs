use clap::Parser;
use config::ReplyConfigEntry;
use matrix_sdk::{
    self,
    config::SyncSettings,
    ruma::{OwnedRoomId, OwnedUserId},
    Client,
};
use serde_json;
use std::{
    str::FromStr,
    sync::{Arc, RwLock},
};
use url::Url;

extern crate pretty_env_logger;
#[macro_use] extern crate log;

mod config;
mod handler;
mod reply;
mod utils;

use reply::ACStrategy;

const INITIAL_DEVICE_DISPLAY_NAME: &str = "Matrix Moderator";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // tracing_subscriber::fmt::init();
    pretty_env_logger::init();

    let args = utils::Args::parse();
    let homeserver_url = args.homeserver;
    let config_path = args.config;
    let room_ids = args
        .rooms
        .into_iter()
        .map(|id| id.try_into().unwrap())
        .collect::<Vec<OwnedRoomId>>();

    let username = args.username;
    let password = args.password;
    let delay = args.delay;
    let cache_file = args.cache_file;
    let allow_users = args.allow_users;
    let vote_room = args.vote_room;
    let vote_delay = args.vote_delay;

    login_and_process_messages(
        homeserver_url,
        username,
        password,
        config_path,
        room_ids,
        delay,
        cache_file,
        allow_users,
        vote_room,
        vote_delay,
    )
    .await?;

    Ok(())
}

async fn login_and_process_messages(
    homeserver_url: String,
    username: String,
    password: String,
    config_path: Vec<String>,
    room_ids: Vec<OwnedRoomId>,
    delay: u64,
    cache_file: Option<String>,
    allow_users: Option<Vec<String>>,
    vote_room: Option<String>,
    vote_delay: u64,
) -> anyhow::Result<()> {
    let homeserver_url = Url::parse(&homeserver_url)?;
    let client = Client::new(homeserver_url).await?;

    let mut cs = vec![];
    for path in config_path {
        let config_file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(config_file);

        let reply_configs: Vec<ReplyConfigEntry> = serde_json::from_reader(reader)?;
        cs.extend(reply_configs.into_iter());
    }
    // let reply_strategy = Arc::new(ACStrategy::new(cs));
    let reply_strategy = Arc::new(RwLock::new(ACStrategy::new(cs)));

    client
        .login_username(&username, &password)
        .initial_device_display_name(INITIAL_DEVICE_DISPLAY_NAME)
        .send()
        .await?;

    // add auto reply handler
    if room_ids.is_empty() {
        handler::add_auto_reply_handler(&client, reply_strategy.clone(), None);
    }
    for room_id in room_ids {
        handler::add_auto_reply_handler(&client, reply_strategy.clone(), Some(&room_id));
    }

    // auto join handler
    client.add_event_handler(handler::auto_join_handler);

    // auto append handler
    let allow_users = allow_users.and_then(|v| {
        Some(
            v.into_iter()
                .map(|id| OwnedUserId::from_str(&id).unwrap())
                .collect(),
        )
    });
    let vote_room = vote_room.and_then(|r| Some(OwnedRoomId::from_str(&r).unwrap()));
    handler::add_auto_append_handle(&client, reply_strategy, delay, cache_file, allow_users, vote_room, vote_delay);

    client.sync(SyncSettings::new()).await?;
    Ok(())
}
