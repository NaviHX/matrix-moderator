use clap::Parser;

#[derive(Parser, Debug)]
pub struct Args {
    #[arg(long)]
    pub homeserver: String,

    #[arg(long, short)]
    pub username: String,

    #[arg(long, short)]
    pub password: String,

    #[arg(long, short)]
    pub config: Vec<String>,

    #[arg(long, short)]
    pub rooms: Vec<String>,

    #[arg(long, short, default_value_t = 6000)]
    pub delay: u64,

    #[arg(long)]
    pub cache_file: Option<String>,

    #[arg(short, long)]
    pub allow_users: Option<Vec<String>>,

    #[arg(long)]
    pub censor_room: Option<String>,
}

