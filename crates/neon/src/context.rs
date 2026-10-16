/// Conservative, byte-based input budget for a local model context. It is an estimate,
/// not a tokenizer; llama.cpp remains the final authority on tokenization.
#[derive(Debug, Clone, Copy)]
pub struct ContextManager {
    pub context_tokens: u32,
    pub response_tokens: u32,
    max_prompt_bytes: usize,
    max_context_bytes: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSelection {
    pub items: Vec<String>,
    pub dropped_items: usize,
    pub dropped_bytes: usize,
}
impl ContextManager {
    pub fn for_model(context_tokens: u32, response_tokens: u32) -> Result<Self, String> {
        let available = context_tokens
            .checked_sub(response_tokens)
            .and_then(|n| n.checked_sub(512))
            .ok_or("configured context cannot reserve response and system budget")?;
        let max_prompt_bytes = (available as usize).saturating_mul(2);
        if max_prompt_bytes < 512 {
            return Err("configured context leaves less than 512 bytes for a prompt".into());
        }
        let max_context_bytes = max_prompt_bytes.saturating_sub(1024).max(256);
        Ok(Self {
            context_tokens,
            response_tokens,
            max_prompt_bytes,
            max_context_bytes,
        })
    }
    pub fn max_prompt_bytes(&self) -> usize {
        self.max_prompt_bytes
    }
    pub fn max_context_bytes(&self) -> usize {
        self.max_context_bytes
    }
    pub fn max_context_items(&self) -> usize {
        12
    }
    pub fn max_recent_turns(&self) -> usize {
        6
    }

    /// Keep higher-priority entries first; callers should order personal memory, RAG,
    /// then recent session turns. Query text is mandatory and never silently dropped.
    pub fn fit_context(
        &self,
        query: &str,
        candidates: impl IntoIterator<Item = String>,
    ) -> Result<Vec<String>, String> {
        Ok(self.select_context(query, candidates)?.items)
    }

    pub fn select_context(
        &self,
        query: &str,
        candidates: impl IntoIterator<Item = String>,
    ) -> Result<ContextSelection, String> {
        if query.trim().is_empty() {
            return Err("context query must not be empty".into());
        }
        if query.len() > self.max_context_bytes {
            return Err(format!(
                "query is {} bytes but context budget is {} bytes",
                query.len(),
                self.max_context_bytes
            ));
        }
        let mut result = Vec::new();
        let mut dropped_items = 0;
        let mut dropped_bytes = 0usize;
        let mut used = query.len();
        for item in candidates {
            if result.len() >= self.max_context_items() {
                dropped_items += 1;
                dropped_bytes = dropped_bytes.saturating_add(item.len());
                continue;
            }
            let next = used.saturating_add(item.len());
            if next > self.max_context_bytes {
                dropped_items += 1;
                dropped_bytes = dropped_bytes.saturating_add(item.len());
                continue;
            }
            used = next;
            result.push(item);
        }
        Ok(ContextSelection {
            items: result,
            dropped_items,
            dropped_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ContextManager;
    #[test]
    fn budget_tracks_context_and_response_and_drops_low_priority_items() {
        let small = ContextManager::for_model(2048, 512).unwrap();
        let large = ContextManager::for_model(4096, 512).unwrap();
        assert!(large.max_context_bytes() > small.max_context_bytes());
        let selection = small
            .select_context(
                "query",
                vec!["memory".into(), "x".repeat(10000), "recent".into()],
            )
            .unwrap();
        assert_eq!(selection.items, vec!["memory", "recent"]);
        assert_eq!(selection.dropped_items, 1);
        assert_eq!(selection.dropped_bytes, 10000);
        assert!(small
            .fit_context(
                &"q".repeat(small.max_context_bytes() + 1),
                Vec::<String>::new()
            )
            .is_err());
    }
    #[test]
    fn rejects_context_that_cannot_reserve_output_and_system_tokens() {
        assert!(ContextManager::for_model(256, 768).is_err());
    }
}
