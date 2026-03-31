use crate::config::LayoutConfig;
use crate::types::Compartment;
use crate::CrateInfo;

/// Group crates into compartments: large crates get their own, small crates
/// are batched together up to the group LOC ceiling.
pub(crate) fn group_into_compartments(
    crates: Vec<CrateInfo>,
    config: &LayoutConfig,
) -> Vec<Compartment> {
    let mut compartments = Vec::new();
    let mut small_group: Vec<CrateInfo> = Vec::new();
    let mut small_group_loc: usize = 0;

    for cr in crates {
        if cr.loc >= config.small_crate_loc {
            // Large crate gets its own compartment.
            compartments.push(Compartment {
                label: cr.name.clone(),
                paths: vec![cr.path],
                estimated_loc: cr.loc,
            });
        } else {
            // Small crate — accumulate into current group.
            if small_group_loc + cr.loc > config.group_loc_ceiling && !small_group.is_empty() {
                compartments.push(flush_small_group(&mut small_group, &mut small_group_loc));
            }
            small_group_loc += cr.loc;
            small_group.push(cr);
        }
    }

    if !small_group.is_empty() {
        compartments.push(flush_small_group(&mut small_group, &mut small_group_loc));
    }

    compartments
}

/// Flush accumulated small crates into a single compartment.
fn flush_small_group(group: &mut Vec<CrateInfo>, loc: &mut usize) -> Compartment {
    let label = group
        .iter()
        .map(|c| c.name.as_str())
        .collect::<Vec<_>>()
        .join(" + ");
    let paths = group.iter().map(|c| c.path.clone()).collect();
    let estimated_loc = *loc;

    group.clear();
    *loc = 0;

    Compartment {
        label,
        paths,
        estimated_loc,
    }
}
