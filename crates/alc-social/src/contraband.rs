//! Contraband economy and discovery events (Directive §7, §8, §17).

use alc_core::{GuardId, PrisonerId, RngStream};
use alc_guard::Guard;
use serde::{Deserialize, Serialize};

/// Categories of contraband circulating in the prison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContrabandItem {
    Weapons,
    Drugs,
    Tools,
    Phones,
    Money,
}

/// A proposed illicit trade between prisoners.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeOffer {
    pub seller: PrisonerId,
    pub buyer: PrisonerId,
    pub item: ContrabandItem,
    pub price: u32,
}

/// A contraband discovery by staff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContrabandFound {
    pub guard: GuardId,
    pub holder: PrisonerId,
    pub item: ContrabandItem,
    pub severity: f32,
}

/// Minimal prisoner profile needed for contraband simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContrabandPrisonerProfile {
    pub id: PrisonerId,
    pub discipline: f32,
    pub risk_tolerance: f32,
    pub manipulation: f32,
    pub aggression: f32,
    pub stimulation: f32,
    pub freedom: f32,
    pub safety: f32,
    pub desperation: f32,
}

/// Generate a simple contraband market snapshot from current prisoners.
///
/// This is an abstract market model: offers emerge from prisoner needs, traits,
/// and opportunism rather than tracked inventories.
pub fn contraband_market(
    prisoners: &[ContrabandPrisonerProfile],
    rng: &mut RngStream,
) -> Vec<TradeOffer> {
    if prisoners.len() < 2 {
        return Vec::new();
    }

    let mut offers = Vec::new();
    let max_offers = ((prisoners.len() / 2).max(1)).min(6);

    for _ in 0..max_offers {
        let si = (rng.next_u64() as usize) % prisoners.len();
        let mut bi = (rng.next_u64() as usize) % prisoners.len();
        if bi == si {
            bi = (bi + 1) % prisoners.len();
        }
        let seller = &prisoners[si];
        let buyer = &prisoners[bi];

        let seller_drive = (seller.manipulation * 0.35
            + seller.risk_tolerance * 0.25
            + (1.0 - seller.discipline) * 0.2
            + seller.freedom * 0.2)
            .clamp(0.0, 1.0);
        let buyer_need = (buyer.stimulation * 0.2
            + buyer.safety * 0.15
            + buyer.freedom * 0.15
            + buyer.desperation * 0.2
            + (1.0 - buyer.discipline) * 0.15
            + buyer.risk_tolerance * 0.15)
            .clamp(0.0, 1.0);

        if seller_drive < 0.35 || buyer_need < 0.35 {
            continue;
        }
        if !rng.chance(((seller_drive * buyer_need) as f64 * 0.6).clamp(0.02, 0.7)) {
            continue;
        }

        let item = pick_item(buyer, rng);
        let base_price = match item {
            ContrabandItem::Weapons => 120,
            ContrabandItem::Drugs => 80,
            ContrabandItem::Tools => 70,
            ContrabandItem::Phones => 100,
            ContrabandItem::Money => 50,
        };
        let scarcity = 0.7 + rng.next_f64() * 0.8;
        let price = ((base_price as f64) * scarcity) as u32;

        offers.push(TradeOffer {
            seller: seller.id,
            buyer: buyer.id,
            item,
            price: price.max(1),
        });
    }

    offers
}

fn pick_item(buyer: &ContrabandPrisonerProfile, rng: &mut RngStream) -> ContrabandItem {
    let r = rng.next_f64() as f32;
    // Bias by needs/traits for a bit of emergence.
    if (buyer.safety > 0.7 || buyer.aggression > 0.7) && r < 0.45 {
        return ContrabandItem::Weapons;
    }
    if buyer.stimulation > 0.7 || buyer.desperation > 0.7 {
        if r < 0.35 {
            return ContrabandItem::Drugs;
        }
        if r < 0.55 {
            return ContrabandItem::Phones;
        }
    }
    match (r * 5.0) as u8 {
        0 => ContrabandItem::Weapons,
        1 => ContrabandItem::Drugs,
        2 => ContrabandItem::Tools,
        3 => ContrabandItem::Phones,
        _ => ContrabandItem::Money,
    }
}

/// Attempt to discover contraband during routine enforcement.
///
/// Returns the most salient discovery found this check, if any.
pub fn contraband_discovery(
    guards: &[Guard],
    prisoners: &[ContrabandPrisonerProfile],
    rng: &mut RngStream,
) -> Option<ContrabandFound> {
    if guards.is_empty() || prisoners.is_empty() {
        return None;
    }

    let mut best: Option<(f64, ContrabandFound)> = None;

    for guard in guards {
        let vigilance = (guard.competence * 0.4
            + guard.integrity * 0.2
            + (1.0 - guard.fatigue) * 0.2
            + (1.0 - guard.stress) * 0.1
            + guard.experience * 0.1)
            .clamp(0.0, 1.0) as f64;
        if vigilance < 0.25 {
            continue;
        }

        for prisoner in prisoners {
            let suspicion = (prisoner.risk_tolerance * 0.25
                + prisoner.manipulation * 0.2
                + (1.0 - prisoner.discipline) * 0.25
                + prisoner.stimulation * 0.15
                + prisoner.freedom * 0.15)
                .clamp(0.0, 1.0) as f64;
            let chance = (vigilance * 0.08 * (0.4 + suspicion)).clamp(0.0, 0.35);
            if !rng.chance(chance) {
                continue;
            }

            let item = pick_item(prisoner, rng);
            let severity = match item {
                ContrabandItem::Weapons => 0.95,
                ContrabandItem::Drugs => 0.75,
                ContrabandItem::Tools => 0.7,
                ContrabandItem::Phones => 0.55,
                ContrabandItem::Money => 0.35,
            };
            let score = chance * (0.5 + severity as f64);
            let found = ContrabandFound {
                guard: guard.id,
                holder: prisoner.id,
                item,
                severity,
            };
            match &best {
                Some((s, _)) if *s >= score => {}
                _ => best = Some((score, found)),
            }
        }
    }

    best.map(|(_, f)| f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alc_core::{GuardId, PrisonerId, RngStream};

    #[test]
    fn contraband_market_and_discovery_can_produce_results() {
        let prisoners: Vec<ContrabandPrisonerProfile> = (1..=8)
            .map(|i| ContrabandPrisonerProfile {
                id: PrisonerId(i),
                discipline: 0.1,
                risk_tolerance: 0.9,
                manipulation: 0.8,
                aggression: 0.6,
                stimulation: 0.9,
                freedom: 0.8,
                safety: 0.8,
                desperation: 0.7,
            })
            .collect();

        let mut offers = Vec::new();
        for seed in 0..30u64 {
            let mut rr = RngStream::new(42, 7 + seed);
            offers = contraband_market(&prisoners, &mut rr);
            if !offers.is_empty() {
                break;
            }
        }
        assert!(!offers.is_empty());

        let mut guards: Vec<Guard> = (1..=2)
            .map(|i| {
                Guard::new(
                    GuardId(i),
                    format!("G{i}"),
                    alc_guard::GuardRank::Officer,
                    alc_guard::PostType::Corridor,
                    &mut RngStream::new(200 + i, i),
                )
            })
            .collect();
        for g in &mut guards {
            g.competence = 0.95;
            g.integrity = 0.9;
            g.fatigue = 0.0;
            g.stress = 0.0;
        }

        let mut found = None;
        for i in 0..80 {
            let mut rr = RngStream::new(999, i);
            found = contraband_discovery(&guards, &prisoners, &mut rr);
            if found.is_some() {
                break;
            }
        }
        assert!(found.is_some());
    }
}
