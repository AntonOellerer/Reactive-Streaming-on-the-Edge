use std::ops::DerefMut;
use std::time::Duration;

use rxrust::impl_helper::*;
use rxrust::impl_local_shared_both;
use rxrust::prelude::*;
use rxrust::scheduler::SpawnHandle;

#[derive(Clone)]
pub struct SlidingWindowWithTimeFunctionOperation<Source, TimeFunction, Scheduler> {
    pub(crate) source: Source,
    pub(crate) window_size: Duration,
    pub(crate) time_function: TimeFunction,
    pub(crate) scheduler: Scheduler,
}

impl<Source, TimeFunction, Scheduler> Observable
    for SlidingWindowWithTimeFunctionOperation<Source, TimeFunction, Scheduler>
where
    Source: Observable,
    TimeFunction: Fn(Source::Item) -> Duration,
{
    type Item = Vec<Source::Item>;
    type Err = Source::Err;
}

#[derive(Clone)]
pub struct SlidingWindowWithTimeFunctionObserver<Observer, Buffer, TimeFunction, Handler> {
    observer: Observer,
    buffer: Buffer,
    time_function: TimeFunction,
    handler: Handler,
}

impl<Obs, Buffer, TimeFunction, Handler, Item, Err> Observer
    for SlidingWindowWithTimeFunctionObserver<Obs, Buffer, TimeFunction, Handler>
where
    Obs: Observer<Item = Vec<Item>, Err = Err>,
    Buffer: RcDerefMut + 'static,
    TimeFunction: Fn(Item) -> Duration + 'static,
    Handler: SubscriptionLike,
    for<'r> Buffer::Target<'r>: DerefMut<Target = Obs::Item>,
{
    type Item = Item;
    type Err = Err;

    fn next(&mut self, value: Self::Item) {
        if !self.handler.is_closed() {
            eprintln!("Adding to buffer");
            self.buffer.rc_deref_mut().push(value);
        }
    }

    fn error(&mut self, err: Self::Err) {
        if !self.handler.is_closed() {
            eprintln!("Error");
            self.handler.unsubscribe();
            self.observer.error(err);
        }
    }

    fn complete(&mut self) {
        if !self.handler.is_closed() {
            eprintln!("Completing");
            let buffer = std::mem::take(&mut *self.buffer.rc_deref_mut());
            if !buffer.is_empty() {
                self.observer.next(buffer);
            }
            self.handler.unsubscribe();
            self.observer.complete();
        }
    }
}

macro_rules! new_sliding_window_observer {
    ($observer:ident, $scheduler: expr, $window_size: expr, $time_function: expr,  $ctx: ident) => {{
        let observer = $ctx::Rc::own($observer);
        let mut observer_c = observer.clone();
        let buffer: $ctx::Rc<Vec<Source::Item>> = $ctx::Rc::own(vec![]);
        let buffer_c = buffer.clone();

        let handler = $scheduler.schedule_repeating(
            move |_| {
                eprintln!("Scanning {:?}", utils::get_now());
                let buffer = &mut *buffer_c.rc_deref_mut();
                if !buffer.is_empty() {
                    buffer.drain_filter(|message| {
                        eprintln!(
                            "{:?} vs {:?}",
                            $time_function(*message) + $window_size,
                            Duration::from_millis(utils::get_now() as u64)
                        );
                        $time_function(*message) + $window_size
                            < Duration::from_millis(utils::get_now() as u64)
                    });
                    eprintln!("Pushing {:?} elements", buffer.len());
                    let copied_buffer = buffer.iter().map(|message| *message).collect();
                    observer_c.next(copied_buffer);
                }
            },
            $window_size / 4,
            None,
        );
        let handler = $ctx::Rc::own(handler);
        SlidingWindowWithTimeFunctionObserver {
            observer,
            buffer,
            time_function: $time_function,
            handler,
        }
    }};
}

impl_local_shared_both! {
    impl<Source, TimeFunction, Scheduler> SlidingWindowWithTimeFunctionOperation<Source, TimeFunction, Scheduler>;
    type Unsub =  TimeSubscription<@ctx::Rc<SpawnHandle>, Source::Unsub>;
    macro method($self: ident, $observer: ident, $ctx: ident) {
        let observer = new_sliding_window_observer!(
            $observer, $self.scheduler, $self.window_size, $self.time_function, $ctx
        );
        let handler = observer.handler.clone();
        let subscription = $self.source.actual_subscribe(observer);
        TimeSubscription{handler, subscription}
    }
    where
        @ctx::local_only('o: 'static,)
        Source: @ctx::Observable,
        Source::Item: @ctx::shared_only(Send + Sync  +) 'static + Clone + Copy,
        Scheduler: @ctx::Scheduler + 'static,
        TimeFunction: Fn(Source::Item) -> Duration + @ctx::shared_only(Send + Sync +) 'static + Copy,
}

pub struct TimeSubscription<H, U> {
    handler: H,
    subscription: U,
}

impl<U: SubscriptionLike, H: SubscriptionLike> SubscriptionLike for TimeSubscription<H, U> {
    fn unsubscribe(&mut self) {
        self.handler.unsubscribe();
        self.subscription.unsubscribe();
    }

    fn is_closed(&self) -> bool {
        self.handler.is_closed()
    }
}

trait HelperTrait: Observable {
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
