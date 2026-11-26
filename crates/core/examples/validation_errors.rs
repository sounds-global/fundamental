//! Example: building and inspecting validation errors with `fundamental_core`.
//!
//! Run it with:
//!     cargo run --example validation_errors -p fundamental_core --all-features

use fundamental_core::Error;
use fundamental_core::validate::{Validate, ValidationContext, ValidationError, ValidationErrors};

struct CreateUserInput {
    email: String,
    name: String,
    age: u8,
}

impl Validate for CreateUserInput {
    fn validate(&self) -> Result<(), ValidationErrors> {
        let mut ctx = ValidationContext::new();

        ctx.field("email").required(&self.email).email(&self.email);

        ctx.field("name")
            .required(&self.name)
            .length(&self.name, 1, 100);

        ctx.field("age").range(self.age, 13, 120);

        ctx.into_result()
    }
}

fn main() {
    let good = CreateUserInput {
        email: "ada@example.com".to_string(),
        name: "Ada".to_string(),
        age: 36,
    };
    println!("== Validating a good input ==");
    match good.validate() {
        Ok(()) => println!("valid! is_valid() = {}", good.is_valid()),
        Err(errors) => print_errors(&errors),
    }

    let bad = CreateUserInput {
        email: "not-an-email".to_string(),
        name: String::new(), // empty -> fails `required`
        age: 5,              // below the minimum of 13
    };
    println!("\n== Validating a bad input ==");
    match bad.validate() {
        Ok(()) => println!("unexpectedly valid"),
        Err(errors) => print_errors(&errors),
    }

    println!("\n== Building ValidationErrors manually ==");
    let mut errors = ValidationErrors::new();

    errors.add(ValidationError::required("title"));
    errors.add(ValidationError::too_short("password", 8, 3));
    errors.add(
        ValidationError::custom("terms_not_accepted", "You must accept the terms")
            .with_developer_message("checkbox `accept_terms` was false"),
    );

    println!("collected {} error(s):", errors.len());
    for error in &errors {
        let field = error.field.as_deref().unwrap_or("<no field>");
        println!("  - [{}] {}: {}", error.code, field, error.message);
        if let Some(dev) = &error.developer_message {
            println!("      (dev: {})", dev);
        }
        for (key, value) in &error.params {
            println!("      param {} = {}", key, value);
        }
    }

    let password_errors = errors.errors_for_field("password");
    println!(
        "\n{} error(s) target the `password` field",
        password_errors.len()
    );

    println!("\n== As a framework Error ==");
    println!("error_code      = {}", errors.error_code());
    println!("display_message = {}", errors.display_message());
    println!("http_status     = {}", errors.http_status_code());
}

fn print_errors(errors: &ValidationErrors) {
    println!("invalid: {} error(s)", errors.len());
    for error in errors {
        let field = error.field.as_deref().unwrap_or("<no field>");
        println!("  - {} ({}): {}", field, error.code, error.message);
    }
}
