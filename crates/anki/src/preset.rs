//! Preset options in janky's own terms, and their mapping to Anki's deck config.
//!
//! [`PresetOptions`] holds the options of one preset. Each option is optional: `None` keeps
//! Anki's default. [`apply`] copies the options that are set into an Anki [`DeckConfig`].
//!
//! The enums [`NewGatherOrder`], [`NewSortOrder`], [`ReviewSortOrder`], [`ReviewMix`] and
//! [`LeechAction`] are the choices of the preset options in `deck.toml`. Their kebab-case
//! variant names are the values that the author writes. They derive `JsonSchema` for the
//! `deck.toml` schema. Some names differ from Anki's own enum names. For example,
//! `ascending-difficulty` is Anki's `EaseDescending`, and `ascending-position` is
//! `LowestPosition`.

use anki::deckconfig::{
    DeckConfig, LeechAction as AnkiLeechAction, NewCardGatherPriority, NewCardSortOrder, ReviewCardOrder,
    ReviewMix as AnkiReviewMix,
};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum NewGatherOrder {
    Deck,
    DeckThenRandomNotes,
    AscendingPosition,
    DescendingPosition,
    RandomNotes,
    RandomCards,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum NewSortOrder {
    Template,
    NoSort,
    TemplateThenRandom,
    RandomNoteThenTemplate,
    RandomCard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewSortOrder {
    Due,
    DueThenDeck,
    DeckThenDue,
    AscendingIntervals,
    DescendingIntervals,
    AscendingDifficulty,
    DescendingDifficulty,
    AscendingRetrievability,
    DescendingRetrievability,
    RelativeOverdueness,
    Random,
    Added,
    ReverseAdded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewMix {
    MixWithReviews,
    AfterReviews,
    BeforeReviews,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LeechAction {
    Suspend,
    TagOnly,
}

/// The options of one [`Preset`](crate::Preset), already converted to Anki's units.
///
/// `None` keeps the value of Anki's default preset.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PresetOptions {
    pub new_per_day: Option<u32>,
    pub reviews_per_day: Option<u32>,
    /// The learning steps in minutes.
    pub learning_steps: Option<Vec<f32>>,
    /// The relearning steps in minutes.
    pub relearning_steps: Option<Vec<f32>>,
    /// The FSRS desired retention as a fraction, such as `0.9`.
    pub desired_retention: Option<f32>,
    /// The maximum review interval in days.
    pub maximum_interval: Option<u32>,
    /// The FSRS parameters. Their number selects the FSRS version: 17 for FSRS 4, 19 for FSRS
    /// 5, and any other number for FSRS 6.
    pub fsrs_params: Option<Vec<f32>>,
    pub new_gather_order: Option<NewGatherOrder>,
    pub new_sort_order: Option<NewSortOrder>,
    pub review_sort_order: Option<ReviewSortOrder>,
    pub new_review_mix: Option<ReviewMix>,
    pub interday_learning_mix: Option<ReviewMix>,
    pub bury_new: Option<bool>,
    pub bury_reviews: Option<bool>,
    pub bury_interday_learning: Option<bool>,
    pub leech_threshold: Option<u32>,
    pub leech_action: Option<LeechAction>,
    /// Play audio automatically when a card side opens. Anki stores the opposite flag.
    pub autoplay: Option<bool>,
    /// Play the question audio again when Anki replays the answer side. Anki stores the
    /// opposite flag.
    pub replay_question_audio: Option<bool>,
    pub show_timer: Option<bool>,
    /// The longest answer time that Anki records, in seconds.
    pub max_answer_seconds: Option<u32>,
}

/// Copies the options of `options` that are set into `config`, and leaves the other values of
/// `config` as they are. Options that Anki stores as the opposite flag are inverted.
pub(crate) fn apply(options: &PresetOptions, config: &mut DeckConfig) {
    let inner = &mut config.inner;
    let set = |target: &mut u32, value: Option<u32>| {
        if let Some(value) = value {
            *target = value;
        }
    };
    set(&mut inner.new_per_day, options.new_per_day);
    set(&mut inner.reviews_per_day, options.reviews_per_day);
    set(&mut inner.maximum_review_interval, options.maximum_interval);
    set(&mut inner.leech_threshold, options.leech_threshold);
    set(&mut inner.cap_answer_time_to_secs, options.max_answer_seconds);
    if let Some(steps) = &options.learning_steps {
        inner.learn_steps = steps.clone();
    }
    if let Some(steps) = &options.relearning_steps {
        inner.relearn_steps = steps.clone();
    }
    if let Some(retention) = options.desired_retention {
        inner.desired_retention = retention;
    }
    if let Some(params) = &options.fsrs_params {
        match params.len() {
            17 => inner.fsrs_params_4 = params.clone(),
            19 => inner.fsrs_params_5 = params.clone(),
            _ => inner.fsrs_params_6 = params.clone(),
        }
    }
    if let Some(order) = options.new_gather_order {
        inner.new_card_gather_priority = match order {
            NewGatherOrder::Deck => NewCardGatherPriority::Deck,
            NewGatherOrder::DeckThenRandomNotes => NewCardGatherPriority::DeckThenRandomNotes,
            NewGatherOrder::AscendingPosition => NewCardGatherPriority::LowestPosition,
            NewGatherOrder::DescendingPosition => NewCardGatherPriority::HighestPosition,
            NewGatherOrder::RandomNotes => NewCardGatherPriority::RandomNotes,
            NewGatherOrder::RandomCards => NewCardGatherPriority::RandomCards,
        } as i32;
    }
    if let Some(order) = options.new_sort_order {
        inner.new_card_sort_order = match order {
            NewSortOrder::Template => NewCardSortOrder::Template,
            NewSortOrder::NoSort => NewCardSortOrder::NoSort,
            NewSortOrder::TemplateThenRandom => NewCardSortOrder::TemplateThenRandom,
            NewSortOrder::RandomNoteThenTemplate => NewCardSortOrder::RandomNoteThenTemplate,
            NewSortOrder::RandomCard => NewCardSortOrder::RandomCard,
        } as i32;
    }
    if let Some(order) = options.review_sort_order {
        inner.review_order = match order {
            ReviewSortOrder::Due => ReviewCardOrder::Day,
            ReviewSortOrder::DueThenDeck => ReviewCardOrder::DayThenDeck,
            ReviewSortOrder::DeckThenDue => ReviewCardOrder::DeckThenDay,
            ReviewSortOrder::AscendingIntervals => ReviewCardOrder::IntervalsAscending,
            ReviewSortOrder::DescendingIntervals => ReviewCardOrder::IntervalsDescending,
            ReviewSortOrder::AscendingDifficulty => ReviewCardOrder::EaseDescending,
            ReviewSortOrder::DescendingDifficulty => ReviewCardOrder::EaseAscending,
            ReviewSortOrder::AscendingRetrievability => ReviewCardOrder::RetrievabilityAscending,
            ReviewSortOrder::DescendingRetrievability => ReviewCardOrder::RetrievabilityDescending,
            ReviewSortOrder::RelativeOverdueness => ReviewCardOrder::RelativeOverdueness,
            ReviewSortOrder::Random => ReviewCardOrder::Random,
            ReviewSortOrder::Added => ReviewCardOrder::Added,
            ReviewSortOrder::ReverseAdded => ReviewCardOrder::ReverseAdded,
        } as i32;
    }
    let mix = |mix: ReviewMix| {
        let value = match mix {
            ReviewMix::MixWithReviews => AnkiReviewMix::MixWithReviews,
            ReviewMix::AfterReviews => AnkiReviewMix::AfterReviews,
            ReviewMix::BeforeReviews => AnkiReviewMix::BeforeReviews,
        };
        value as i32
    };
    if let Some(value) = options.new_review_mix {
        inner.new_mix = mix(value);
    }
    if let Some(value) = options.interday_learning_mix {
        inner.interday_learning_mix = mix(value);
    }
    let flag = |target: &mut bool, value: Option<bool>| {
        if let Some(value) = value {
            *target = value;
        }
    };
    flag(&mut inner.bury_new, options.bury_new);
    flag(&mut inner.bury_reviews, options.bury_reviews);
    flag(&mut inner.bury_interday_learning, options.bury_interday_learning);
    flag(&mut inner.show_timer, options.show_timer);
    if let Some(autoplay) = options.autoplay {
        inner.disable_autoplay = !autoplay;
    }
    if let Some(replay) = options.replay_question_audio {
        inner.skip_question_when_replaying_answer = !replay;
    }
    if let Some(action) = options.leech_action {
        inner.leech_action = match action {
            LeechAction::Suspend => AnkiLeechAction::Suspend,
            LeechAction::TagOnly => AnkiLeechAction::TagOnly,
        } as i32;
    }
}

/// The keys of the options in `options` that would change `config` (see [`apply`]), with the
/// names that `deck.toml` uses.
pub(crate) fn changed(options: &PresetOptions, config: &DeckConfig) -> Vec<&'static str> {
    let none = PresetOptions::default;
    let o = options.clone();
    let each = [
        ("new_per_day", PresetOptions { new_per_day: o.new_per_day, ..none() }),
        (
            "reviews_per_day",
            PresetOptions {
                reviews_per_day: o.reviews_per_day,
                ..none()
            },
        ),
        (
            "learning_steps",
            PresetOptions {
                learning_steps: o.learning_steps,
                ..none()
            },
        ),
        (
            "relearning_steps",
            PresetOptions {
                relearning_steps: o.relearning_steps,
                ..none()
            },
        ),
        (
            "desired_retention",
            PresetOptions {
                desired_retention: o.desired_retention,
                ..none()
            },
        ),
        (
            "maximum_interval",
            PresetOptions {
                maximum_interval: o.maximum_interval,
                ..none()
            },
        ),
        ("fsrs_params", PresetOptions { fsrs_params: o.fsrs_params, ..none() }),
        (
            "new_gather_order",
            PresetOptions {
                new_gather_order: o.new_gather_order,
                ..none()
            },
        ),
        (
            "new_sort_order",
            PresetOptions {
                new_sort_order: o.new_sort_order,
                ..none()
            },
        ),
        (
            "review_sort_order",
            PresetOptions {
                review_sort_order: o.review_sort_order,
                ..none()
            },
        ),
        (
            "new_review_mix",
            PresetOptions {
                new_review_mix: o.new_review_mix,
                ..none()
            },
        ),
        (
            "interday_learning_mix",
            PresetOptions {
                interday_learning_mix: o.interday_learning_mix,
                ..none()
            },
        ),
        ("bury_new", PresetOptions { bury_new: o.bury_new, ..none() }),
        ("bury_reviews", PresetOptions { bury_reviews: o.bury_reviews, ..none() }),
        (
            "bury_interday_learning",
            PresetOptions {
                bury_interday_learning: o.bury_interday_learning,
                ..none()
            },
        ),
        (
            "leech_threshold",
            PresetOptions {
                leech_threshold: o.leech_threshold,
                ..none()
            },
        ),
        ("leech_action", PresetOptions { leech_action: o.leech_action, ..none() }),
        ("autoplay", PresetOptions { autoplay: o.autoplay, ..none() }),
        (
            "replay_question_audio",
            PresetOptions {
                replay_question_audio: o.replay_question_audio,
                ..none()
            },
        ),
        ("show_timer", PresetOptions { show_timer: o.show_timer, ..none() }),
        (
            "max_answer_seconds",
            PresetOptions {
                max_answer_seconds: o.max_answer_seconds,
                ..none()
            },
        ),
    ];
    each.into_iter()
        .filter(|(_, one)| {
            let mut copy = config.clone();
            apply(one, &mut copy);
            copy.inner != config.inner
        })
        .map(|(name, _)| name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_option_is_checked_for_changes() {
        let options = PresetOptions {
            new_per_day: Some(1),
            reviews_per_day: Some(2),
            learning_steps: Some(vec![3.0]),
            relearning_steps: Some(vec![4.0]),
            desired_retention: Some(0.75),
            maximum_interval: Some(6),
            fsrs_params: Some(vec![0.5; 21]),
            new_gather_order: Some(NewGatherOrder::RandomCards),
            new_sort_order: Some(NewSortOrder::RandomCard),
            review_sort_order: Some(ReviewSortOrder::Random),
            new_review_mix: Some(ReviewMix::BeforeReviews),
            interday_learning_mix: Some(ReviewMix::AfterReviews),
            bury_new: Some(true),
            bury_reviews: Some(true),
            bury_interday_learning: Some(true),
            leech_threshold: Some(7),
            leech_action: Some(LeechAction::Suspend),
            autoplay: Some(false),
            replay_question_audio: Some(false),
            show_timer: Some(true),
            max_answer_seconds: Some(8),
        };
        assert_eq!(changed(&options, &DeckConfig::default()).len(), 21);
        let mut config = DeckConfig::default();
        apply(&options, &mut config);
        assert!(changed(&options, &config).is_empty());
    }
}
