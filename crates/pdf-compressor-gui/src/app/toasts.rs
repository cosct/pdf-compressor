use super::notices::Notice;

#[derive(Debug, Clone)]
pub struct Toast {
    pub id: u64,
    pub notice: Notice,
    pub paused: bool,
}

#[derive(Debug, Default)]
pub struct Toasts {
    items: Vec<Toast>,
    next_id: u64,
}

impl Toasts {
    pub fn push(&mut self, notice: Notice) -> u64 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let id = self.next_id;
        self.items.push(Toast {
            id,
            notice,
            paused: false,
        });
        id
    }

    pub fn dismiss(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|toast| toast.id != id);
        self.items.len() != before
    }

    pub fn pause(&mut self, id: u64) -> bool {
        let Some(toast) = self.items.iter_mut().find(|toast| toast.id == id) else {
            return false;
        };
        toast.paused = true;
        true
    }

    pub fn resume(&mut self, id: u64) -> bool {
        let Some(toast) = self.items.iter_mut().find(|toast| toast.id == id) else {
            return false;
        };
        toast.paused = false;
        true
    }

    pub fn items(&self) -> &[Toast] {
        &self.items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::notices::NoticeTone;
    use std::collections::BTreeMap;

    fn notice() -> Notice {
        Notice {
            code: "compress.warning.outputNotSmaller".into(),
            tone: NoticeTone::Warning,
            values: BTreeMap::new(),
            fallback: "No smaller output.".into(),
        }
    }

    #[test]
    fn dismiss_pause_and_resume_are_scoped_by_toast_id() {
        let mut toasts = Toasts::default();
        let first = toasts.push(notice());
        let second = toasts.push(notice());
        assert!(toasts.pause(first));
        assert!(toasts.items()[0].paused);
        assert!(toasts.resume(first));
        assert!(!toasts.items()[0].paused);
        assert!(toasts.dismiss(first));
        assert_eq!(toasts.items().len(), 1);
        assert_eq!(toasts.items()[0].id, second);
        assert!(!toasts.dismiss(first));
    }
}
