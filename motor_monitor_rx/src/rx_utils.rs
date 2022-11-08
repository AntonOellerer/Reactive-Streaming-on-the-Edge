use std::time::Duration;

use rxrust::prelude::*;

use crate::sliding_window::SlidingWindowWithTimeFunctionOperation;

pub trait HelperTrait: Observable {
    fn sliding_window<TimeFunction, Scheduler>(
        self,
        window_size: Duration,
        time_function: TimeFunction,
        scheduler: Scheduler,
    ) -> SlidingWindowWithTimeFunctionOperation<Self, TimeFunction, Scheduler>
    where
        TimeFunction: Fn(Self::Item) -> Duration;
}

impl<T> HelperTrait for T
where
    T: Observable,
{
    fn sliding_window<TimeFunction, Scheduler>(
        self,
        window_size: Duration,
        time_function: TimeFunction,
        scheduler: Scheduler,
    ) -> SlidingWindowWithTimeFunctionOperation<Self, TimeFunction, Scheduler>
    where
        TimeFunction: Fn(Self::Item) -> Duration,
    {
        SlidingWindowWithTimeFunctionOperation {
            source: self,
            window_size,
            time_function,
            scheduler,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use futures::executor::LocalPool;
    use futures::executor::ThreadPool;
    use rxrust::prelude::*;

    use crate::rx_utils::HelperTrait;

    #[test]
    fn it_shall_make_a_window_local() {
        let mut local = LocalPool::new();
        let expected = vec![vec![0, 1, 2]];
        let actual = Rc::new(RefCell::new(vec![]));
        let actual_c = actual.clone();

        from_iter(0..3)
            .map(|i| (i, Duration::from_secs(1)))
            .sliding_window(
                Duration::from_secs(1),
                |(_, duration)| duration,
                local.spawner(),
            )
            .map(|window| window.iter().map(|(i, _)| *i).collect::<Vec<i32>>())
            .subscribe(move |vec| actual_c.borrow_mut().push(vec));

        local.run();

        // this can't be really tested as local scheduler runs on a single thread
        assert_eq!(expected, *actual.borrow());
    }

    #[test]
    fn it_shall_not_block_on_error_local() {
        let mut local = LocalPool::new();

        create(|subscriber| {
            subscriber.next(0);
            subscriber.next(1);
            subscriber.next(2);
            subscriber.error(());
        })
        .map(|i| (i, Duration::from_secs(1)))
        .sliding_window(
            Duration::from_secs(1),
            |(_, duration)| duration,
            local.spawner(),
        )
        .subscribe(|_| {});

        // if this call blocks execution, the observer's handle has not been
        // unsubscribed
        local.run();
    }

    #[test]
    fn it_shall_make_a_window_shared() {
        let pool = ThreadPool::new().unwrap();

        let expected = vec![
            vec![0],
            vec![0, 1],
            vec![0, 1],
            vec![1, 2],
            vec![1, 2],
            vec![2, 3],
            vec![2, 3],
            vec![2, 3],
        ];
        let actual = Arc::new(Mutex::new(vec![]));
        let actual_c = actual.clone();

        let is_completed = Arc::new(AtomicBool::new(false));
        let is_completed_c = is_completed.clone();

        create(|subscriber| {
            let sleep = Duration::from_millis(100);
            subscriber.next(0);
            std::thread::sleep(sleep);
            subscriber.next(1);
            std::thread::sleep(sleep);
            subscriber.next(2);
            std::thread::sleep(sleep);
            subscriber.next(3);
            std::thread::sleep(sleep);
            subscriber.complete();
        })
        .map(|i| (i, Duration::from_millis(utils::get_now() as u64)))
        .sliding_window(Duration::from_millis(210), |(_, duration)| duration, pool)
        .map(|window| window.iter().map(|(i, _)| *i).collect::<Vec<i32>>())
        .into_shared()
        .subscribe_all(
            move |vec| {
                let mut a = actual_c.lock().unwrap();
                (*a).push(vec);
            },
            |()| {},
            move || is_completed_c.store(true, Ordering::Relaxed),
        );

        std::thread::sleep(Duration::from_millis(450));
        assert_eq!(expected, *actual.lock().unwrap());
        assert!(is_completed.load(Ordering::Relaxed));
    }

    #[test]
    fn it_shall_not_emit_window_on_error() {
        let pool = ThreadPool::new().unwrap();

        let expected = vec![vec![0, 1, 2]];
        let actual = Arc::new(Mutex::new(vec![]));
        let actual_c = actual.clone();

        let error_called = Arc::new(AtomicBool::new(false));
        let error_called_c = error_called.clone();

        create(|subscriber| {
            let sleep = Duration::from_millis(100);
            subscriber.next(0);
            subscriber.next(1);
            subscriber.next(2);
            std::thread::sleep(sleep);
            subscriber.next(3);
            subscriber.next(4);
            subscriber.error(());
        })
        .map(|i| (i, Duration::from_millis(utils::get_now() as u64)))
        .sliding_window(Duration::from_millis(210), |(_, duration)| duration, pool)
        .map(|window| window.iter().map(|(i, _)| *i).collect::<Vec<i32>>())
        .into_shared()
        .subscribe_all(
            move |vec| {
                let mut a = actual_c.lock().unwrap();
                (*a).push(vec);
            },
            move |_| error_called_c.store(true, Ordering::Relaxed),
            || {},
        );
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(expected, *actual.lock().unwrap());
        assert!(error_called.load(Ordering::Relaxed));
    }
}
