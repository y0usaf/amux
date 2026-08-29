//! Shared usage report types. Both usage readers (pi JSONL and fx
//! usage.jsonl) aggregate into these; keeping the definitions in one file
//! lets harness-core compile only the fx reader while still exporting the
//! same report shapes.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PiUsageTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub total_cost: f64,
}

impl PiUsageTotals {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.cache_creation_tokens)
            .saturating_add(self.cache_read_tokens)
    }

    pub(crate) fn add(&mut self, other: &Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cache_creation_tokens = self
            .cache_creation_tokens
            .saturating_add(other.cache_creation_tokens);
        self.cache_read_tokens = self
            .cache_read_tokens
            .saturating_add(other.cache_read_tokens);
        self.total_cost += other.total_cost;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PiUsageModelBreakdown {
    pub model_name: String,
    pub totals: PiUsageTotals,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PiUsageDay {
    pub date: String,
    pub totals: PiUsageTotals,
    pub models_used: Vec<String>,
    pub model_breakdowns: Vec<PiUsageModelBreakdown>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PiUsageReport {
    pub days: Vec<PiUsageDay>,
    pub totals: PiUsageTotals,
    pub files_scanned: usize,
    pub entries: usize,
    pub skipped_duplicates: usize,
}
