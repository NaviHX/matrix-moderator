use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ReplyType {
    PlainMessage(String),
}

use aho_corasick::AhoCorasick;
use std::collections::HashMap;

pub struct ACStrategy {
    pub ac_automaton: AhoCorasick,
    pub patterns: Vec<String>,
    pub pattern_reply_map: HashMap<String, Vec<ReplyType>>,
}

use crate::config::ReplyConfigEntry;

impl ACStrategy {
    pub fn find_reply(&self, msg: &str) -> Vec<ReplyType> {
        let mut replies = vec![];

        for mat in self.ac_automaton.find_iter(msg) {
            let id = mat.pattern().as_usize();
            let pattern = &self.patterns[id];

            replies.extend(
                self.pattern_reply_map
                    .get(pattern)
                    .unwrap()
                    .iter()
                    .map(|r| r.clone()),
            )
        }

        replies
    }
    pub fn new(config_entries: Vec<ReplyConfigEntry>) -> Self {
        let mut patterns: Vec<String> = vec![];
        let mut pattern_reply_map = HashMap::new();

        for ReplyConfigEntry {
            patterns: ps,
            reply: r,
        } in config_entries
        {
            for p in ps {
                pattern_reply_map
                    .entry(p.clone())
                    .and_modify(|replies: &mut Vec<ReplyType>| {
                        replies.push(r.clone());
                    })
                    .or_insert_with(|| {
                        patterns.push(p.clone());
                        vec![r.clone()]
                    });
            }
        }

        Self {
            ac_automaton: AhoCorasick::new(patterns.clone()).unwrap(),
            patterns,
            pattern_reply_map,
        }
    }
}

