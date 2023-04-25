use matrix_sdk::{
    room::Room,
    ruma::{
        events::room::message::{
            MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent
        },
        events::room::member::StrippedRoomMemberEvent,
        RoomId,
    },
    Client,
};
use tokio::time::{sleep, Duration};

use crate::reply::{ACStrategy, ReplyType};
use rand;
use std::sync::Arc;

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

                let replies = reply_strategy.find_reply(&text_content.body);

                let replies: Vec<String> = replies
                    .into_iter()
                    .filter(|r| {
                        // filter non-text reply
                        // HACK always true because it has only one enum member
                        true
                    })
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
    reply_strategy: Arc<ACStrategy>,
    room_id: Option<&RoomId>,
) {
    if let Some(room_id) = room_id {
        client.add_room_event_handler(room_id, BASE_REPLY!(reply_strategy));
    } else {
        client.add_event_handler(BASE_REPLY!(reply_strategy));
    }
}


pub async fn auto_join_handler(
    room_member: StrippedRoomMemberEvent,
    client: Client,
    room: Room,
) {
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
                eprintln!("Failed to join room {} ({err:?}), retrying in {delay}s", room.room_id());

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
