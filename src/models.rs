use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
pub struct Item {
    id: Option<u64>,
    name: String,
    description: String,
    image: String,
    location: u64,
    tags: Vec<u64>,
    amount: u64,
    properties_internal: Vec<Property>,
    properties_custom: Vec<Property>,
    attachments: HashMap<String, String>,
    last_edited: u64,
    created: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Property {
    id: u64,
    name: String,
    value: String,
    display_type: Option<String>,
    min: Option<u64>,
    max: Option<u64>,
}
