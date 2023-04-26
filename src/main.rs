use clap::Parser;
use config::ReplyConfigEntry;
use matrix_sdk::{self, config::SyncSettings, ruma::OwnedRoomId, Client};
use serde_json;
use std::sync::{Arc, RwLock};
use url::Url;

mod config;
mod handler;
mod reply;
mod utils;

use reply::ACStrategy;

const INITIAL_DEVICE_DISPLAY_NAME: &str = "Matrix Moderator";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

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

    login_and_process_messages(
        homeserver_url,
        username,
        password,
        config_path,
        room_ids,
        delay,
        cache_file,
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
    handler::add_auto_append_handle(&client, reply_strategy, delay, cache_file);

    client.sync(SyncSettings::new()).await?;
    Ok(())
}
