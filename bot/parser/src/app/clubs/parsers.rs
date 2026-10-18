/// This module defines traits for parsing match and seat information from HTML content, which can be implemented by specific parsers for different clubs or websites.
/// The `ParseMatch` trait provides a method for parsing match information and returning a list of `Encounter` instances,
/// while the `ParseSeat` trait provides a method for parsing seat information and returning a list of `Seat` instances based on the provided HTML content and encounter information.
use crate::core::encounter::Encounter;
use crate::core::seat::Seat;

pub trait ParseMatch {
    fn parse_match(&self, html: &str) -> Vec<Encounter>;
}

pub trait ParseSeat {
    fn parse_seat(&self, html: &str, encounter: Encounter) -> Vec<Seat>;
}
