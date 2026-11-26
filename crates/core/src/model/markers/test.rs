//! Mock marker-trait implementations for the model marker tests.
//!
//! Hand-written: `#[derive(Model)]` lives in `fundamental_macros`, which depends on this crate.

use super::{BoundTo, Database};
use crate::{Model, Sealed};

/// Mock [`Model`] implementation used in marker-trait tests.
pub struct TestModel;

/// Mock [`Database`] marker used in marker-trait tests.
#[derive(Debug)]
pub struct TestDb;

impl Sealed for TestDb {}
impl Database for TestDb {
    const NAME: &'static str = "TestDB";
    const SUPPORTS_TRANSACTIONS: bool = true;
    const SUPPORTS_JOINS: bool = true;
    const SUPPORTS_SECONDARY_INDEXES: bool = true;
}

impl Model for TestModel {
    const TYPE_NAME: &'static str = "TestModel";
}

impl BoundTo<TestDb> for TestModel {}
