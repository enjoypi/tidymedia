use std::collections::HashMap;
use crate::{Media, media};
use crate::media::Checksum;


pub struct MediaIndex {
    files: HashMap<Checksum, Media>,
}