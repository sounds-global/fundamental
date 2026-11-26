//! The authorization schema.

use fundamental::Namespace;

/// A person.
#[derive(Namespace)]
pub enum User {}

/// A team.
#[derive(Namespace)]
pub enum Team {
    /// A member of the team.
    #[relation(direct(User))]
    Member,
}
