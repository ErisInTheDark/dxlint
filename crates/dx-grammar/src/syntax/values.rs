use std::ops::Range;

pub struct ValueSource<'a> {
    pub text: &'a str,
    pub source: &'a str,
    extent: Range<usize>,
    starts: Vec<usize>,
    spans: Option<Vec<Range<usize>>>,
}

impl<'a> ValueSource<'a> {
    pub fn new(text: &'a str, source: &'a str, extent: Range<usize>, spans: Option<&[Range<usize>]>) -> Result<Self, String> {
        if text.len() > u32::MAX as usize {
            return Err("mapped text exceeds parser byte offsets".into());
        }
        if source.get(extent.clone()).is_none() {
            return Err("mapped source extent is outside source or a UTF-8 boundary".into());
        }
        let mut starts = text.char_indices().map(|(offset, _)| offset).collect::<Vec<_>>();
        if let Some(spans) = spans {
            if spans.len() != starts.len() {
                return Err("mapped source needs one span per Unicode scalar".into());
            }
            let mut previous = extent.start..extent.start;
            for span in spans {
                if span.start < previous.start || span.end < previous.end
                    || span.start < extent.start || span.end > extent.end
                    || source.get(span.clone()).is_none()
                {
                    return Err("mapped scalar spans must stay ordered within the source extent and UTF-8 boundaries".into());
                }
                previous = span.clone();
            }
        }
        starts.push(text.len());
        Ok(Self { text, source, extent, starts, spans: spans.map(<[_]>::to_vec) })
    }

    pub fn range(&self, start: u32, end: u32) -> Result<Range<usize>, String> {
        let (start, end) = (start as usize, end as usize);
        if self.text.get(start..end).is_none() {
            return Err("mapped node span is outside text or a UTF-8 boundary".into());
        }
        let Some(spans) = &self.spans else { return Ok(self.extent.clone()) };
        let from = self.starts.binary_search(&start).map_err(|_| "mapped node start is not a Unicode-scalar boundary")?;
        let to = self.starts.binary_search(&end).map_err(|_| "mapped node end is not a Unicode-scalar boundary")?;
        if from == to {
            let anchor = spans.get(from).map_or(self.extent.end, |span| span.start);
            return Ok(anchor..anchor);
        }
        Ok(spans[from].start..spans[to - 1].end)
    }
}

#[cfg(test)]
mod tests {
    use super::ValueSource;

    #[test]
    fn scalar_ranges_keep_escape_and_multibyte_source_extents() {
        let source = r"prefix '\x61🐾b' suffix";
        let begin = source.find('\\').unwrap();
        let paw = source.find('🐾').unwrap();
        let end = paw + '🐾'.len_utf8() + 1;
        let spans = [begin..paw, paw..end - 1, end - 1..end];
        let mapped = ValueSource::new("a🐾b", source, begin..end, Some(&spans)).unwrap();
        assert_eq!(&source[mapped.range(0, 1).unwrap()], r"\x61");
        assert_eq!(&source[mapped.range(1, 5).unwrap()], "🐾");
        assert_eq!(&source[mapped.range(0, 5).unwrap()], r"\x61🐾");
        assert_eq!(mapped.range(1, 1).unwrap(), paw..paw);
        assert_eq!(mapped.range(6, 6).unwrap(), end..end);
        assert!(mapped.range(2, 5).is_err());
        assert!(mapped.range(5, 1).is_err());
    }

    #[test]
    fn computed_values_and_overlapping_spans_have_distinct_mapping_rules() {
        let source = "factory()";
        let computed = ValueSource::new("ab", source, 0..source.len(), None).unwrap();
        assert_eq!(computed.range(0, 1).unwrap(), 0..source.len());
        assert_eq!(computed.range(1, 1).unwrap(), 0..source.len());
        let spans = [0..source.len(), 0..source.len()];
        let mapped = ValueSource::new("ab", source, 0..source.len(), Some(&spans)).unwrap();
        assert_eq!(mapped.range(0, 2).unwrap(), 0..source.len());
        assert_eq!(mapped.range(1, 1).unwrap(), 0..0);
    }

    #[test]
    fn invalid_maps_are_rejected_before_any_partial_tree_is_published() {
        let source = "🐾abc";
        for spans in [
            vec![0..4],
            vec![0..1, 4..5],
            vec![0..4, 5..9],
            vec![4..5, 0..4],
        ] {
            assert!(ValueSource::new("ab", source, 0..source.len(), Some(&spans)).is_err());
        }
        assert!(ValueSource::new("a", source, 1..4, None).is_err());
        assert!(ValueSource::new("a", source, 0..source.len() + 1, None).is_err());
    }
}
