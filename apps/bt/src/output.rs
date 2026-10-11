//! An outcome as `bt` prints it: the text of its report, or with `--json` the outcome itself with
//! its `schema` field (`10-agents.md`, §9).

use bt_script::{Outcome, to_json_pretty};

use crate::exit::{Printed, Status};

/// `outcome` printed as `--json` asks, ending as `status` says.
pub(crate) fn printed<O: Outcome>(
    outcome: &O,
    json: bool,
    status: Status,
) -> anyhow::Result<Printed> {
    let text = if json {
        let mut text = to_json_pretty(outcome)?;
        text.push('\n');
        text
    } else {
        outcome.report().text()
    };
    Ok(Printed {
        text,
        status,
        failure: None,
    })
}
