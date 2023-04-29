use matrix_sdk::{
    room::Room,
    ruma::{
        events::reaction::OriginalSyncReactionEvent,
        events::room::member::StrippedRoomMemberEvent,
        events::room::message::{
            MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
        },
        OwnedEventId, OwnedRoomId, OwnedUserId, RoomId,
    },
    Client,
};
use serde_json::json;
use tokio;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

use crate::reply::{ACStrategy, ReplyType};
use rand;
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::prelude::*;
use std::sync::{Arc, Mutex, RwLock};

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

                    info!("Send reply: {}", reply);
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
        info!("Create auto reply handler in room {room_id}");
        client.add_room_event_handler(room_id, BASE_REPLY!(reply_strategy));
    } else {
        info!("Create auto reply handler for all rooms");
        client.add_event_handler(BASE_REPLY!(reply_strategy));
    }
}

pub async fn auto_join_handler(room_member: StrippedRoomMemberEvent, client: Client, room: Room) {
    if room_member.state_key != client.user_id().unwrap() {
        return;
    }

    if let Room::Invited(room) = room {
        tokio::spawn(async move {
            let mut delay = 2;

            while let Err(err) = room.accept_invitation().await {
                // retry autojoin due to synapse sending invites, before the
                // invited user can join for more information see
                // https://github.com/matrix-org/synapse/issues/4345
                error!(
                    "Failed to join room {} ({err:?}), retrying in {delay}s",
                    room.room_id()
                );

                sleep(Duration::from_secs(delay)).await;
                delay *= 2;

                if delay > 3600 {
                    error!("Can't join room {} ({err:?})", room.room_id());
                    break;
                }
            }
        });
    }
}

#[derive(Debug)]
struct EntryUpdate {
    pub pattern: String,
    pub reply: String,
}

#[derive(Debug, Clone)]
struct VoteReply {
    pub pattern: String,
    pub reply: String,
    pub vote: i32,
}

pub fn add_auto_append_handle(
    client: &Client,
    reply_strategy: Arc<RwLock<ACStrategy>>,
    delay: u64,
    cache_file: Option<String>,
    allow_users: Option<Vec<OwnedUserId>>,
    vote_room: Option<OwnedRoomId>,
    vote_delay: u64,
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

                        info!("Construct new pattern-reply: {pattern} -> {reply}");
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
                        });
                    info!("New AhoCorasick Automaton constructed");
                }
            }
        });
    }

    // allowed user set
    let userid_set = if let Some(users) = allow_users {
        users.into_iter().collect::<HashSet<_>>()
    } else {
        HashSet::new()
    };
    let userid_set = Arc::new(userid_set);
    // vote map
    let vote_id_map = HashMap::<OwnedEventId, VoteReply>::new();
    let vote_id_map = Arc::new(Mutex::new(vote_id_map));

    client.add_event_handler({
        let vote_id_map = vote_id_map.clone();
        let vote_room = vote_room.clone();
        move |event: OriginalSyncRoomMessageEvent, client: Client| {
            let tx = tx.clone();
            let userid_set = userid_set.clone();
            let vote_room = vote_room.clone();
            let vote_id_map = vote_id_map.clone();

            async move {
                let MessageType::Text(text_content) = &event.content.msgtype else { return; };
                let sender_id = &event.sender.clone();

                if text_content.body.starts_with("/append ") {
                    let s: String = text_content.body.chars().skip(8).collect();
                    let arr: Vec<&str> = s.splitn(2, " ").collect();
                    if arr.len() == 2 {
                        if userid_set.get(sender_id).is_some() {
                            info!("Receive new pattern-reply from allowed user {sender_id}: {} -> {}", arr[0], arr[1]);
                            tx.send(EntryUpdate {
                                pattern: arr[0].to_owned(),
                                reply: arr[1].to_owned(),
                            })
                            .await
                            .unwrap();
                        } else {
                            if let Some(Room::Joined(room)) =
                                vote_room.and_then(|roomid| client.get_room(&roomid))
                            {
                                info!("Receive new pattern-reply from not-allowed user {sender_id}: {} -> {}", arr[0], arr[1]);
                                let message = RoomMessageEventContent::text_plain(format!(
                                    "Sender: {}\n{} -> {}",
                                    sender_id, arr[0], arr[1]
                                ));
                                let vote_id = room.send(message, None).await.unwrap().event_id;
                                info!("Launch a new vote for {} -> {}: {vote_id}", arr[0], arr[1]);
                                vote_id_map.lock().unwrap().insert(
                                    vote_id.clone(),
                                    VoteReply {
                                        pattern: arr[0].to_owned(),
                                        reply: arr[1].to_owned(),
                                        vote: 0,
                                    },
                                );

                                tokio::spawn(async move {
                                    sleep(Duration::from_secs(vote_delay)).await;

                                    let vote = vote_id_map.lock().unwrap().get(&vote_id).and_then(|v| Some(v.clone()));
                                    if let Some(VoteReply {
                                        pattern,
                                        reply,
                                        vote,
                                    }) = vote
                                    {
                                        info!("Vote Result for {vote_id} [{pattern} -> {reply}]: {vote}");
                                        if vote > 0 {
                                            tx.send(EntryUpdate {
                                                pattern: pattern.clone(),
                                                reply: reply.clone(),
                                            })
                                            .await
                                            .unwrap();
                                        }
                                    }

                                    vote_id_map.lock().unwrap().remove(&vote_id).unwrap();
                                });
                            }
                        }
                    }
                }
            }
        }
    });

    // reaction vote handler
    if let Some(roomid) = vote_room {
        client.add_room_event_handler(&roomid, {
            let vote_id_map = vote_id_map.clone();
            move |reaction: OriginalSyncReactionEvent| {
                let vote_id_map = vote_id_map.clone();
                async move {
                    let vote_id = reaction.content.relates_to.event_id;
                    if let Some(vote_reaction) = get_vote(reaction.content.relates_to.key) {
                        vote_id_map.lock().unwrap().entry(vote_id.clone()).and_modify(|e| {
                            info!("Reaction for {vote_id}: {vote_reaction:?}");
                            e.vote += match vote_reaction {
                                Vote::Yes => 1,
                                Vote::No => -1,
                            }
                        });
                    }
                }
            }
        });
    }
}

#[derive(Debug)]
enum Vote {
    Yes,
    No,
}

fn get_vote(key: String) -> Option<Vote> {
    if key.contains("👍") {
        Some(Vote::Yes)
    } else if key.contains("👎") {
        Some(Vote::No)
    } else {
        None
    }
}
