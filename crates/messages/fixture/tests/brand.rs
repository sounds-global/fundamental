//! A message says the brand with `:brand`, from the brand terms the process installed once.

use fundamental_messages::{BrandTerm, BrandTerms, BrandTermsInstalled, install_brand_terms};
use fundamental_messages_fixture::m;

struct Sounds;

impl BrandTerms for Sounds {
    fn term(&self, term: BrandTerm) -> &str {
        match term {
            BrandTerm::Name => "Sounds.Global",
            BrandTerm::LegalName => "Sounds Global B.V.",
            BrandTerm::SiteDomain => "sounds.global",
            BrandTerm::PlatformDomain => "sounds.app",
            _ => "",
        }
    }
}

struct Other;

impl BrandTerms for Other {
    fn term(&self, _term: BrandTerm) -> &str {
        "Other"
    }
}

/// The process's brand: installed by whichever test runs first.
fn installed() {
    let _ = install_brand_terms(Sounds);
}

#[test]
fn a_message_says_the_installed_brand_in_each_language() {
    installed();
    assert_eq!(m::brand::signed().format_in("en"), "Sent by Sounds.Global");
    assert_eq!(
        m::brand::signed().format_in("nl"),
        "Verstuurd door Sounds.Global"
    );
    assert_eq!(
        m::brand::footer().format_in("en"),
        "Sounds Global B.V. · sounds.global"
    );
}

#[test]
fn a_second_brand_is_refused_and_the_first_stays() {
    installed();
    assert!(matches!(
        install_brand_terms(Other),
        Err(BrandTermsInstalled::Already)
    ));
    assert_eq!(m::brand::signed().format_in("en"), "Sent by Sounds.Global");
}

#[test]
fn every_term_is_named_as_a_message_names_it() {
    for term in BrandTerm::ALL {
        assert_eq!(BrandTerm::named(term.name()), Some(term));
    }
    assert_eq!(BrandTerm::named("nme"), None);
}
