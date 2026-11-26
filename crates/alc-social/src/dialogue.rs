//! Specific conversation topics and selection for co-presence encounters.

use alc_core::PrisonerId;
use alc_prisoner::{Needs, Traits};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DialogueTopic {
    Work,
    Family,
    Gang,
    Escape,
    Complaints,
    Humor,
    Threats,
    Recruitment,
    Gossip,
    Advice,
}

impl DialogueTopic {
    pub fn as_str(self) -> &'static str {
        match self {
            DialogueTopic::Work => "work",
            DialogueTopic::Family => "family",
            DialogueTopic::Gang => "gang life",
            DialogueTopic::Escape => "escape plans",
            DialogueTopic::Complaints => "complaints",
            DialogueTopic::Humor => "humor",
            DialogueTopic::Threats => "threats",
            DialogueTopic::Recruitment => "recruitment",
            DialogueTopic::Gossip => "gossip",
            DialogueTopic::Advice => "advice",
        }
    }
}

/// Select a conversation topic from the pair's traits, needs, and relationship.
pub fn select_topic(
    traits_a: &Traits,
    traits_b: &Traits,
    needs_a: &Needs,
    needs_b: &Needs,
    friendship: f32,
    hostility: f32,
    gang_a: Option<u64>,
    gang_b: Option<u64>,
) -> DialogueTopic {
    if hostility > 0.45 {
        return DialogueTopic::Threats;
    }
    if gang_a.is_some() && gang_a == gang_b && traits_a.leadership > 0.6 {
        return DialogueTopic::Recruitment;
    }
    if needs_a.family_contact > 0.6 || needs_b.family_contact > 0.6 {
        return DialogueTopic::Family;
    }
    if needs_a.freedom > 0.6 || needs_b.freedom > 0.6 {
        return DialogueTopic::Escape;
    }
    if needs_a.companionship > 0.6 || needs_b.companionship > 0.6 {
        return DialogueTopic::Gossip;
    }
    if needs_a.purpose > 0.6 || needs_b.purpose > 0.6 {
        return DialogueTopic::Work;
    }
    if friendship > 0.5 {
        if traits_a.sociability > 0.6 || traits_b.sociability > 0.6 {
            return DialogueTopic::Humor;
        }
        return DialogueTopic::Advice;
    }
    if needs_a.stimulation > 0.6 || needs_b.stimulation > 0.6 {
        return DialogueTopic::Complaints;
    }
    DialogueTopic::Gossip
}

/// A short conversation record for history/UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub participants: Vec<PrisonerId>,
    pub topic: DialogueTopic,
    pub summary: String,
}