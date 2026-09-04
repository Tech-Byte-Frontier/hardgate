use anyhow::{Result, bail};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(crate) struct RecordDetails {
    functions: FunctionDetails,
    branches: BranchDetails,
}

pub(crate) struct DetailValidation<'a> {
    pub seen_counts: &'a HashSet<&'static str>,
    pub functions_found: usize,
    pub functions_hit: usize,
    pub branches_found: usize,
    pub branches_hit: usize,
    pub require_functions: bool,
    pub require_branches: bool,
}

impl RecordDetails {
    pub(crate) fn ingest_fn(&mut self, rest: &str) -> Result<()> {
        let (line, name) = parse_function_record(rest)?;
        let line = parse_line(line, "FN", rest)?;
        let name = parse_name(name, "FN", rest)?;
        if self.functions.declarations.insert(name, line).is_some() {
            bail!("Duplicate LCOV FN function name");
        }
        Ok(())
    }

    pub(crate) fn ingest_fnda(&mut self, rest: &str) -> Result<()> {
        let (hits, name) = rest
            .split_once(',')
            .ok_or_else(|| anyhow::anyhow!("Malformed LCOV FNDA metric `{rest}`"))?;
        let hits = hits
            .trim()
            .parse::<usize>()
            .map_err(|_| anyhow::anyhow!("Malformed LCOV FNDA hit count `{hits}`"))?;
        let name = parse_name(name, "FNDA", rest)?;
        if self.functions.hits.insert(name, hits).is_some() {
            bail!("Duplicate LCOV FNDA function name");
        }
        Ok(())
    }

    pub(crate) fn ingest_brda(&mut self, rest: &str) -> Result<()> {
        let (line, rest) = rest
            .split_once(',')
            .ok_or_else(|| anyhow::anyhow!("Malformed LCOV BRDA metric `{rest}`"))?;
        let (block, rest) = rest
            .split_once(',')
            .ok_or_else(|| anyhow::anyhow!("Malformed LCOV BRDA metric `{rest}`"))?;
        let (branch, taken) = rest
            .rsplit_once(',')
            .ok_or_else(|| anyhow::anyhow!("Malformed LCOV BRDA metric `{rest}`"))?;
        let line = parse_line(line, "BRDA", rest)?;
        let block = parse_branch_field(block, "block", rest)?;
        let branch = parse_branch_field(branch, "branch", rest)?;
        let taken = parse_taken(taken, rest)?;
        let key = BranchKey {
            line,
            block,
            branch,
        };
        if self.branches.values.insert(key, taken).is_some() {
            bail!("Duplicate LCOV BRDA branch identity");
        }
        Ok(())
    }

    pub(crate) fn validate(&self, input: DetailValidation<'_>) -> Result<()> {
        validate_functions(&self.functions, &input)?;
        validate_branches(&self.branches, &input)
    }
}

#[derive(Default)]
struct FunctionDetails {
    declarations: HashMap<String, usize>,
    hits: HashMap<String, usize>,
}

#[derive(Default)]
struct BranchDetails {
    values: HashMap<BranchKey, Option<usize>>,
}

#[derive(Debug, Hash, PartialEq, Eq)]
struct BranchKey {
    line: usize,
    block: String,
    branch: String,
}

fn parse_line(value: &str, tag: &str, rest: &str) -> Result<usize> {
    let line = value
        .trim()
        .parse::<usize>()
        .map_err(|_| anyhow::anyhow!("Malformed LCOV {tag} line number `{value}`"))?;
    if line == 0 {
        bail!("LCOV {tag} line number must be greater than zero");
    }
    if rest.contains('\0') {
        bail!("Malformed LCOV {tag} metric");
    }
    Ok(line)
}

fn parse_function_record(rest: &str) -> Result<(&str, &str)> {
    let (line, tail) = rest
        .split_once(',')
        .ok_or_else(|| anyhow::anyhow!("Malformed LCOV FN metric `{rest}`"))?;
    let Some((end, name)) = tail.split_once(',') else {
        return Ok((line, tail));
    };
    if end.trim().parse::<usize>().is_err() {
        return Ok((line, tail));
    }
    let start = parse_line(line, "FN", rest)?;
    let end = parse_line(end, "FN", rest)?;
    if end < start {
        bail!("Malformed LCOV FN range `{rest}`");
    }
    Ok((line, name))
}

fn parse_name(value: &str, tag: &str, rest: &str) -> Result<String> {
    let name = value.trim();
    if name.is_empty() || name.contains('\0') {
        bail!("Malformed LCOV {tag} function name in `{rest}`");
    }
    Ok(name.to_string())
}

fn parse_branch_field(value: &str, field: &str, rest: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty()
        || value.contains('\0')
        || value
            .split(',')
            .any(|part| part.trim().is_empty() || part.trim() == "-")
    {
        bail!("Malformed LCOV BRDA {field} field in `{rest}`");
    }
    Ok(value.to_string())
}

fn parse_taken(value: &str, rest: &str) -> Result<Option<usize>> {
    let value = value.trim();
    if value == "-" {
        return Ok(None);
    }
    value
        .parse::<usize>()
        .map(Some)
        .map_err(|_| anyhow::anyhow!("Malformed LCOV BRDA taken count `{rest}`"))
}

fn validate_functions(details: &FunctionDetails, input: &DetailValidation<'_>) -> Result<()> {
    if details.declarations.len() != details.hits.len()
        || details
            .declarations
            .keys()
            .any(|name| !details.hits.contains_key(name))
    {
        bail!("LCOV FN/FNDA function identities do not match");
    }
    let aggregate = input.seen_counts.contains("FNF");
    if (!details.declarations.is_empty() && !aggregate && !input.require_functions)
        || (aggregate || input.require_functions)
            && (details.declarations.len() != input.functions_found
                || hit_count(details.hits.values()) != input.functions_hit)
    {
        bail!("LCOV FN/FNDA details require matching FNF/FNH counts");
    }
    Ok(())
}

fn validate_branches(details: &BranchDetails, input: &DetailValidation<'_>) -> Result<()> {
    let aggregate = input.seen_counts.contains("BRF");
    if (!details.values.is_empty() && !aggregate && !input.require_branches)
        || (aggregate || input.require_branches)
            && (details.values.len() != input.branches_found
                || hit_count(details.values.values().filter_map(Option::as_ref))
                    != input.branches_hit)
    {
        bail!("LCOV BRDA details require matching BRF/BRH counts");
    }
    Ok(())
}

fn hit_count<'a, I>(values: I) -> usize
where
    I: IntoIterator<Item = &'a usize>,
{
    values.into_iter().filter(|hits| **hits > 0).count()
}
