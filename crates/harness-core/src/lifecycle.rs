//! Native lifecycle component selectors. Combined activation remains unfinished.
#![cfg(windows)]

use std::io;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Component {
    Core,
    CodeTools,
    Subscriptions,
    TokenWorkflow,
}

impl Component {
    pub fn flag(self) -> &'static str {
        match self {
            Self::Core => "--core-only",
            Self::CodeTools => "--code-tools-only",
            Self::Subscriptions => "--subscriptions-only",
            Self::TokenWorkflow => "--token-workflow-only",
        }
    }
}

pub fn parse_flag(name: &str) -> Option<Component> {
    match name {
        "--core-only" => Some(Component::Core),
        "--code-tools-only" => Some(Component::CodeTools),
        "--subscriptions-only" => Some(Component::Subscriptions),
        "--token-workflow-only" => Some(Component::TokenWorkflow),
        _ => None,
    }
}

pub fn exclusive(current: Option<Component>, next: Component) -> io::Result<Component> {
    match current {
        None => Ok(next),
        Some(existing) if existing == next => {
            Err(io::Error::other("duplicate native installation option"))
        }
        Some(_) => Err(io::Error::other(
            "Component selectors are mutually exclusive.",
        )),
    }
}

pub fn required(component: Option<Component>) -> io::Result<Component> {
    component.ok_or_else(|| {
        io::Error::other(
            "native lifecycle requires an explicit component selector; combined activation has not completed native acceptance",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_are_mutually_exclusive() {
        let selected = exclusive(None, Component::Core).unwrap();
        let error = exclusive(Some(selected), Component::CodeTools).unwrap_err();
        assert!(error.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn combined_activation_is_not_implied() {
        let error = required(None).unwrap_err();
        assert!(error.to_string().contains("explicit component selector"));
    }
}
