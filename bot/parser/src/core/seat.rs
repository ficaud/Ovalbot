/// SeatComposition represents the details of a seat's location and category, including the category, access type, row, and seat number.
#[derive(Debug, Clone)]
pub struct SeatComposition {
    pub category: String,
    pub bloc: String,
    pub row: String,
    pub seat_number: u64,
}

/// SeatInfo contains the full name of the seat and its composition details.
#[derive(Debug, Clone)]
pub struct SeatInfo {
    pub full_name: String,
    pub composition: SeatComposition,
    pub preview_url: Option<String>,
}

/// SeatId uniquely identifies a resale seat on the ticketing site:
/// the pack the seat belongs to and the ticket it refers to inside that pack.
#[derive(Debug, Clone)]
pub struct SeatId {
    pub pack_id: u64,
    pub ticket_id: String,
}

/// Seat represents a seat available for a rugby match, including its information and its unique identity on the ticketing site.
#[derive(Debug, Clone)]
pub struct Seat {
    pub seat_info: SeatInfo,
    pub id: SeatId,
    pub price: String,
}
