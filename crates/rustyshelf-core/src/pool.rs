//! Work shared out over a few threads at once.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// `f` on each item in turn (with its index), `workers` threads at a time; a thread stops taking items once `f`
/// returns false.
pub fn each<T: Sync>(items: &[T], workers: usize, f: impl Fn(usize, &T) -> bool + Sync) {
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..workers.min(items.len()) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(t) = items.get(i) else { return };
                if !f(i, t) {
                    return;
                }
            });
        }
    });
}

/// `f` of each item, `workers` threads at a time, in the items' order.
pub fn map<T: Sync, R: Send>(items: &[T], workers: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let got: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
    each(items, workers, |i, t| {
        let r = f(t);
        *got[i].lock().unwrap() = Some(r);
        true
    });
    got.into_iter().map(|g| g.into_inner().unwrap().expect("every item worked on")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_keeps_the_order() {
        let items: Vec<usize> = (0..50).collect();
        assert_eq!(map(&items, 4, |n| n * 2), (0..50).map(|n| n * 2).collect::<Vec<_>>());
        assert!(map(&[] as &[usize], 4, |n| *n).is_empty());
    }

    #[test]
    fn each_stops_a_thread_that_says_so() {
        let seen = AtomicUsize::new(0);
        each(&[1, 2, 3, 4, 5], 1, |_, n| {
            seen.fetch_add(1, Ordering::Relaxed);
            *n < 3
        });
        assert_eq!(seen.into_inner(), 3);
    }
}
