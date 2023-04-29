use matrix_sdk::{
    room::Room,
    ruma::{
        events::room::member::StrippedRoomMemberEvent,
        events::room::message::{
            MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
        },
        OwnedRoomId, OwnedUserId, RoomId,
    },
    Client,
};
use serde_json::json;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

use crate::reply::{ACStrategy, ReplyType};
use rand;
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::prelude::*;
use std::sync::{Arc, RwLock};

macro_rules! BASE_REPLY {
    ($name:ident) => {
        move |event: OriginalSyncRoomMessageEvent, room: Room, client: Client| {
            let reply_strategy = $name.clone();

            async move {
                let Room::Joined(room) = room else { return; };
                let room_id = room.room_id();
                let MessageType::Text(text_content) = &event.content.msgtype else { return; };

                if client.user_id().unwrap() == event.sender {
                    return;
                }

                let replies = reply_strategy
                    .read()
                    .unwrap()
                    .find_reply(&text_content.body);

                let replies: Vec<String> = replies
                    .into_iter()
                    // .filter(|r| {
                    //     // filter non-text reply
                    //     // HACK always true because it has only one enum member
                    //     true
                    // })
                    .map(|r| match r {
                        ReplyType::PlainMessage(r) => r,
                    })
                    .collect::<Vec<_>>();

                if !replies.is_empty() {
                    let len = replies.len();
                    let target = rand::random::<usize>() % len;
                    let reply = replies[target].clone();

                    let content = RoomMessageEventContent::text_plain(reply)
                        .make_reply_to(&event.into_full_event(room_id.to_owned()));
                    room.send(content, None).await.unwrap();
                }
            }
        }
    };
}

pub fn add_auto_reply_handler(
    client: &Client,
    reply_strategy: Arc<RwLock<ACStrategy>>,
    room_id: Option<&RoomId>,
) {
    if let Some(room_id) = room_id {
        client.add_room_event_handler(room_id, BASE_REPLY!(reply_strategy));
    } else {
        client.add_event_handler(BASE_REPLY!(reply_strategy));
    }
}

pub async fn auto_join_handler(room_member: StrippedRoomMemberEvent, client: Client, room: Room) {
    if room_member.state_key != client.user_id().unwrap() {
        return;
    }

    if let Room::Invited(room) = room {
        tokio::spawn(async move {
            println!("Autojoining room {}", room.room_id());
            let mut delay = 2;

            while let Err(err) = room.accept_invitation().await {
                // retry autojoin due to synapse sending invites, before the
                // invited user can join for more information see
                // https://github.com/matrix-org/synapse/issues/4345
                eprintln!(
                    "Failed to join room {} ({err:?}), retrying in {delay}s",
                    room.room_id()
                );

                sleep(Duration::from_secs(delay)).await;
                delay *= 2;

                if delay > 3600 {
                    eprintln!("Can't join room {} ({err:?})", room.room_id());
                    break;
                }
            }
            println!("Successfully joined room {}", room.room_id());
        });
    }
}

#[derive(Debug)]
struct EntryUpdate {
    pub pattern: String,
    pub reply: String,
}

pub fn add_auto_append_handle(
    client: &Client,
    reply_strategy: Arc<RwLock<ACStrategy>>,
    delay: u64,
    cache_file: Option<String>,
    allow_users: Option<Vec<OwnedUserId>>,
    censor_room: Option<OwnedRoomId>,
) {
    let (tx, mut rx) = mpsc::channel::<EntryUpdate>(256);

    {
        let reply_strategy = reply_strategy.clone();

        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(delay)).await;

                let mut new_patterns = vec![];
                let mut new_replies = vec![];
                {
                    let mut append_file = cache_file.clone().and_then(|f| {
                        Some(
                            OpenOptions::new()
                                .create(true)
                                .write(true)
                                .append(true)
                                .open(&f)
                                .unwrap(),
                        )
                    });

                    while let Ok(EntryUpdate { pattern, reply }) = rx.try_recv() {
                        append_file.as_mut().and_then(|f| {
                            writeln!(
                                *f,
                                "{}",
                                serde_json::to_string(&json!({
                                    "pattern": pattern,
                                    "reply": reply,
                                }))
                                .unwrap()
                            )
                            .unwrap();
                            Option::<()>::None
                        });

                        new_patterns.push(pattern);
                        new_replies.push(reply);
                    }
                }

                if new_patterns.len() > 0 && new_replies.len() > 0 {
                    let mut mut_ref = reply_strategy.write().unwrap();
                    mut_ref.patterns.extend_from_slice(&new_patterns);
                    mut_ref.ac_automaton =
                        aho_corasick::AhoCorasick::new(mut_ref.patterns.clone()).unwrap();

                    new_patterns
                        .into_iter()
                        .zip(new_replies.into_iter().map(ReplyType::PlainMessage))
                        .for_each(|(p, r)| {
                            mut_ref
                                .pattern_reply_map
                                .entry(p)
                                .and_modify(|replies| replies.push(r.clone()))
                                .or_insert_with(|| vec![r.clone()]);
                        })
                }
            }
        });
    }

    let userid_set = if let Some(users) = allow_users {
        users.into_iter().collect::<HashSet<_>>()
    } else {
        HashSet::new()
    };
    let userid_set = Arc::new(userid_set);

    client.add_event_handler(move |event: OriginalSyncRoomMessageEvent, client: Client| {
        let tx = tx.clone();
        let userid_set = userid_set.clone();
        let censor_room = censor_room.clone();

        async move {
            let MessageType::Text(text_content) = &event.content.msgtype else { return; };
            let sender_id = &event.sender.clone();

            if text_content.body.starts_with("/append ") {
                if userid_set.is_empty() || userid_set.get(sender_id).is_some() {
                    let s: String = text_content.body.chars().skip(8).collect();
                    let arr: Vec<&str> = s.splitn(2, " ").collect();

                    if arr.len() == 2 {
                        tx.send(EntryUpdate {
                            pattern: arr[0].to_owned(),
                            reply: arr[1].to_owned(),
                        })
                        .await
                        .unwrap();
                    }
                } else {
                    if let Some(roomid) = censor_room {
                        if let Some(Room::Joined(room)) = client.get_room(&roomid) {
                            let message = RoomMessageEventContent::text_plain(format!("Sender: {}\n{}", sender_id, text_content.body));
                            room.send(message, None).await.unwrap();
                        }
                    }
                }
            }
        }
    });
}
