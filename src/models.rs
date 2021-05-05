use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
pub struct Item {
    pub id: Option<u64>,
    pub name: String,
    pub description: String,
    pub image: String,
    pub location: u64,
    pub tags: Vec<u64>,
    pub amount: u64,
    pub properties_internal: Vec<Property>,
    pub properties_custom: Vec<Property>,
    pub attachments: HashMap<String, String>,
    pub last_edited: u64,
    pub created: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Property {
    pub id: u64,
    pub name: String,
    pub value: String,
    pub display_type: Option<String>,
    pub min: Option<u64>,
    pub max: Option<u64>,
}
