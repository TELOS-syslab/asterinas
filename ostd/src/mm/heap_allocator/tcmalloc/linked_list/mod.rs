//! Provide the intrusive LinkedList

mod bounded_list;
mod elastic_list;

pub use bounded_list::BoundedList;
pub use elastic_list::ElasticList;