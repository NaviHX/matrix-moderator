use matrix_sdk::{
    room::Room,
    ruma::{
        events::room::message::{
            MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
        },
        RoomId,
    },
    Client,
};

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
