use std::io::{self, Write};

use anyhow::anyhow;
use clap::Parser;
use config::ReplyConfigEntry;
use matrix_sdk::{
    self,
    config::SyncSettings,
    ruma::{
        api::client::session::get_login_types::v3::{IdentityProvider, LoginType},
        OwnedRoomId,
    },
    Client,
};
use serde_json;
use std::sync::Arc;
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

    login_and_process_messages(homeserver_url, username, password, config_path, room_ids).await?;

    Ok(())
}

async fn login_and_process_messages(
    homeserver_url: String,
    username: String,
    password: String,
    config_path: Vec<String>,
    room_ids: Vec<OwnedRoomId>,
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
    let reply_strategy = Arc::new(ACStrategy::new(cs));

    let mut login_choices = Vec::new();
    let login_types = client.get_login_types().await?.flows;

    for login_type in login_types {
        match login_type {
            LoginType::Password(_) => {
                login_choices.push(LoginChoice::Password);
            }
            // LoginType::Sso(sso) => {
            //     if sso.identity_providers.is_empty() {
            //         login_choices.push(LoginChoice::Sso);
            //     } else {
            //         login_choices
            //             .extend(sso.identity_providers.into_iter().map(LoginChoice::SsoIdp));
            //     }
            // }
            _ => {}
        }
    }

    match login_choices.len() {
        0 => {
            return Err(anyhow!(
                "Homeserver login types incompatibale with this client"
            ))
        }
        1 | _ => {
            client.login_username(&username, &password)
                .initial_device_display_name(INITIAL_DEVICE_DISPLAY_NAME)
                .send()
                .await?;
        }
        // 1 | _ => login_choices[0].login(&client).await?,
        // _ => offer_choices_and_login(&client, login_choices).await?,
    }

    if room_ids.is_empty() {
        handler::add_auto_reply_handler(&client, reply_strategy.clone(), None);
    }

    for room_id in room_ids {
        handler::add_auto_reply_handler(&client, reply_strategy.clone(), Some(&room_id));
    }

    client.sync(SyncSettings::new()).await?;
    Ok(())
}

#[derive(Debug)]
enum LoginChoice {
    Password,
    Sso,
    SsoIdp(IdentityProvider),
}

impl LoginChoice {
    async fn login(&self, client: &Client) -> anyhow::Result<()> {
        match self {
            LoginChoice::Password => login_with_password(client).await,
            LoginChoice::Sso => login_with_sso(client, None).await,
            LoginChoice::SsoIdp(idp) => login_with_sso(client, Some(idp)).await,
        }
    }
}

impl std::fmt::Display for LoginChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoginChoice::Password => write!(f, "Username and password"),
            LoginChoice::Sso => write!(f, "SSO"),
            LoginChoice::SsoIdp(idp) => write!(f, "SSO via {}", idp.name),
        }
    }
}

async fn offer_choices_and_login(client: &Client, choices: Vec<LoginChoice>) -> anyhow::Result<()> {
    println!("Several Options are available to login with this home server:\n");

    let choice = loop {
        for (idx, login_choice) in choices.iter().enumerate() {
            println!("{idx}) {login_choice}");
        }

        println!("\n Enter your choice: ");
        io::stdout().flush().expect("Unable to write to stdout");
        let mut choice_str = String::new();
        io::stdin()
            .read_line(&mut choice_str)
            .expect("Unable to read user input");

        match choice_str.trim().parse::<usize>() {
            Ok(choice) => {
                if choice >= choices.len() {
                    eprintln!("This is not a valid choice");
                } else {
                    break choice;
                }
            }
            Err(_) => eprintln!("This is not a valid choice. Try again.\n"),
        };
    };

    choices[choice].login(client).await
}

async fn login_with_password(client: &Client) -> anyhow::Result<()> {
    println!("Logging in with username and password...");

    loop {
        print!("\nUsername: ");
        io::stdout().flush().expect("Unable to write to stdout");
        let mut username = String::new();
        io::stdin()
            .read_line(&mut username)
            .expect("Unable to read user input");
        username = username.trim().to_owned();

        print!("Password: ");
        io::stdout().flush().expect("Unable to write to stdout");
        let mut password = String::new();
        io::stdin()
            .read_line(&mut password)
            .expect("Unable to read user input");
        password = password.trim().to_owned();

        match client
            .login_username(&username, &password)
            .initial_device_display_name(INITIAL_DEVICE_DISPLAY_NAME)
            .send()
            .await
        {
            Ok(_) => {
                println!("Logged in as {username}");
                break;
            }
            Err(e) => {
                println!("Error logging in: {e}");
                println!("Please try again\n");
            }
        }
    }

    Ok(())
}

async fn login_with_sso(client: &Client, idp: Option<&IdentityProvider>) -> anyhow::Result<()> {
    println!("Logging in with SSO...");

    let mut login_builder = client.login_sso(|url| async move {
        println!("\nOpen this URL in your browser: {url}\n");
        println!("Waiting for login token...");
        Ok(())
    });

    if let Some(idp) = idp {
        login_builder = login_builder.identity_provider_id(&idp.id);
    }

    login_builder.send().await?;

    println!("Logged in as {}", client.user_id().unwrap());

    Ok(())
}
