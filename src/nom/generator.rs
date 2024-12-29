use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug)]
pub struct IdGenerator {
    counter: AtomicU64,
    prefix: &'static str,
}

impl IdGenerator {
    pub fn new(prefix: &'static str) -> Self {
        Self { counter: AtomicU64::new(0), prefix }
    }

    pub fn next_id(&self) -> String {
        let id = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("{}_{}", self.prefix, id)
    }
}

#[cfg(test)]
mod test {
    use crate::nom::generator::IdGenerator;

    #[test]
    fn test_id_generator() {
        let id_gen = IdGenerator::new("test");
        assert_eq!(id_gen.next_id(), "test_0");
        assert_eq!(id_gen.next_id(), "test_1");
    }
}
