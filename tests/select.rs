//! Tests for the `select!` macro.

#![deny(unsafe_code)]

extern crate crossbeam;
#[macro_use]
extern crate crossbeam_channel as channel;

use std::any::Any;
use std::thread;
use std::time::Duration;

use channel::TryRecvError;

// TODO: modify a borrowed sender/receiver inside select! body
// TODO: fix unreachable lints in select!
// TODO: use abortguard so that panicking message prints a more sensible message
// TODO: remove all instances of std::process

fn ms(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

#[test]
fn smoke1() {
    let (s1, r1) = channel::unbounded::<usize>();
    let (s2, r2) = channel::unbounded::<usize>();

    s1.send(1).unwrap();

    select! {
        recv(r1) -> v => assert_eq!(v, Ok(1)),
        recv(r2) -> _ => panic!(),
    }

    s2.send(2).unwrap();

    select! {
        recv(r1) -> _ => panic!(),
        recv(r2) -> v => assert_eq!(v, Ok(2)),
    }
}

#[test]
fn smoke2() {
    let (_s1, r1) = channel::unbounded::<i32>();
    let (_s2, r2) = channel::unbounded::<i32>();
    let (_s3, r3) = channel::unbounded::<i32>();
    let (_s4, r4) = channel::unbounded::<i32>();
    let (s5, r5) = channel::unbounded::<i32>();

    s5.send(5).unwrap();

    select! {
        recv(r1) -> _ => panic!(),
        recv(r2) -> _ => panic!(),
        recv(r3) -> _ => panic!(),
        recv(r4) -> _ => panic!(),
        recv(r5) -> v => assert_eq!(v, Ok(5)),
    }
}

#[test]
fn closed() {
    let (s1, r1) = channel::unbounded::<i32>();
    let (s2, r2) = channel::unbounded::<i32>();

    crossbeam::scope(|scope| {
        scope.spawn(|| {
            drop(s1);
            thread::sleep(ms(500));
            s2.send(5).unwrap();
        });

        select! {
            recv(r1) -> v => assert!(v.is_err()),
            recv(r2) -> _ => panic!(),
            default(ms(1000)) => panic!(),
        }

        r2.recv().unwrap();
    });

    select! {
        recv(r1) -> v => assert!(v.is_err()),
        recv(r2) -> _ => panic!(),
        default(ms(1000)) => panic!(),
    }

    crossbeam::scope(|scope| {
        scope.spawn(|| {
            thread::sleep(ms(500));
            drop(s2);
        });

        select! {
            recv(r2) -> v => assert!(v.is_err()),
            default(ms(1000)) => panic!(),
        }
    });
}

#[test]
fn default() {
    let (s1, r1) = channel::unbounded::<i32>();
    let (s2, r2) = channel::unbounded::<i32>();

    select! {
        recv(r1) -> _ => panic!(),
        recv(r2) -> _ => panic!(),
        default => {}
    }

    drop(s1);

    select! {
        recv(r1) -> v => assert!(v.is_err()),
        recv(r2) -> _ => panic!(),
        default => panic!(),
    }

    s2.send(2).unwrap();

    select! {
        recv(r2) -> v => assert_eq!(v, Ok(2)),
        default => panic!(),
    }

    select! {
        recv(r2) -> _ => panic!(),
        default => {},
    }

    select! {
        default => {},
    }
}

#[test]
fn timeout() {
    let (_s1, r1) = channel::unbounded::<i32>();
    let (s2, r2) = channel::unbounded::<i32>();

    crossbeam::scope(|scope| {
        scope.spawn(|| {
            thread::sleep(ms(1500));
            s2.send(2).unwrap();
        });

        select! {
            recv(r1) -> _ => panic!(),
            recv(r2) -> _ => panic!(),
            default(ms(1000)) => {},
        }

        select! {
            recv(r1) -> _ => panic!(),
            recv(r2) -> v => assert_eq!(v, Ok(2)),
            default(ms(1000)) => panic!(),
        }
    });

    crossbeam::scope(|scope| {
        let (s, r) = channel::unbounded::<i32>();

        scope.spawn(move || {
            thread::sleep(ms(500));
            drop(s);
        });

        select! {
            default(ms(1000)) => {
                select! {
                    recv(r) -> v => assert!(v.is_err()),
                    default => panic!(),
                }
            }
        }
    });
}

#[test]
fn default_when_closed() {
    let (_, r) = channel::unbounded::<i32>();

    select! {
        recv(r) -> v => assert!(v.is_err()),
        default => panic!(),
    }

    let (_, r) = channel::unbounded::<i32>();

    select! {
        recv(r) -> v => assert!(v.is_err()),
        default(ms(1000)) => panic!(),
    }
}

// TODO: default when sender closed

#[test]
fn unblocks() {
    let (s1, r1) = channel::bounded::<i32>(0);
    let (s2, r2) = channel::bounded::<i32>(0);

    crossbeam::scope(|scope| {
        scope.spawn(|| {
            thread::sleep(ms(500));
            s2.send(2).unwrap();
        });

        select! {
            recv(r1) -> _ => panic!(),
            recv(r2) -> v => assert_eq!(v, Ok(2)),
            default(ms(1000)) => panic!(),
        }
    });

    crossbeam::scope(|scope| {
        scope.spawn(|| {
            thread::sleep(ms(500));
            assert_eq!(r1.recv().unwrap(), 1);
        });

        select! {
            send(s1, 1) -> _ => {},
            send(s2, 2) -> _ => panic!(),
            default(ms(1000)) => panic!(),
        }
    });
}

#[test]
fn both_ready() {
    let (s1, r1) = channel::bounded(0);
    let (s2, r2) = channel::bounded(0);

    crossbeam::scope(|scope| {
        scope.spawn(|| {
            thread::sleep(ms(500));
            s1.send(1).unwrap();
            assert_eq!(r2.recv().unwrap(), 2);
        });

        for _ in 0..2 {
            select! {
                recv(r1) -> v => assert_eq!(v, Ok(1)),
                send(s2, 2) -> _ => {},
            }
        }
    });
}

#[test]
fn loop_try() {
    const RUNS: usize = 20;

    for _ in 0..RUNS {
        let (s1, r1) = channel::bounded::<i32>(0);
        let (s2, r2) = channel::bounded::<i32>(0);
        let (s_end, r_end) = channel::bounded::<()>(0);

        crossbeam::scope(|scope| {
            scope.spawn(|| {
                loop {
                    select! {
                        send(s1, 1) -> _ => break,
                        default => {}
                    }

                    select! {
                        recv(r_end) -> _ => break,
                        default => {}
                    }
                }
            });

            scope.spawn(|| {
                loop {
                    if let Ok(x) = r2.try_recv() {
                        assert_eq!(x, 2);
                        break;
                    }

                    select! {
                        recv(r_end) -> _ => break,
                        default => {}
                    }
                }
            });

            scope.spawn(|| {
                thread::sleep(ms(500));

                select! {
                    recv(r1) -> v => assert_eq!(v, Ok(1)),
                    send(s2, 2) -> _ => {},
                    default(ms(500)) => panic!(),
                }

                drop(s_end);
            });
        });
    }
}

#[test]
fn cloning1() {
    crossbeam::scope(|scope| {
        let (s1, r1) = channel::unbounded::<i32>();
        let (_s2, r2) = channel::unbounded::<i32>();
        let (s3, r3) = channel::unbounded::<()>();

        scope.spawn(move || {
            r3.recv().unwrap();
            drop(s1.clone());
            assert_eq!(r3.try_recv(), Err(TryRecvError::Empty));
            s1.send(1).unwrap();
            r3.recv().unwrap();
        });

        s3.send(()).unwrap();

        select! {
            recv(r1) -> _ => {},
            recv(r2) -> _ => {},
        }

        s3.send(()).unwrap();
    });
}

#[test]
fn cloning2() {
    let (s1, r1) = channel::unbounded::<()>();
    let (s2, r2) = channel::unbounded::<()>();
    let (_s3, _r3) = channel::unbounded::<()>();

    crossbeam::scope(|scope| {
        scope.spawn(move || {
            select! {
                recv(r1) -> _ => panic!(),
                recv(r2) -> _ => {},
            }
        });

        thread::sleep(ms(500));
        drop(s1.clone());
        s2.send(()).unwrap();
    })
}

#[test]
fn preflight1() {
    let (s, r) = channel::unbounded();
    s.send(()).unwrap();

    select! {
        recv(r) -> _ => {}
    }
}

#[test]
fn preflight2() {
    let (s, r) = channel::unbounded();
    drop(s.clone());
    s.send(()).unwrap();
    drop(s);

    select! {
        recv(r) -> v => assert!(v.is_ok()),
    }
    assert_eq!(r.try_recv(), Err(TryRecvError::Disconnected));
}

#[test]
fn preflight3() {
    let (s, r) = channel::unbounded();
    drop(s.clone());
    s.send(()).unwrap();
    drop(s);
    r.recv().unwrap();

    select! {
        recv(r) -> v => assert!(v.is_err())
    }
}

#[test]
fn duplicate_cases() {
    let (s, r) = channel::unbounded::<i32>();
    let mut hit = [false; 4];

    while hit.iter().any(|hit| !hit) {
        select! {
            recv(r) -> _ => hit[0] = true,
            recv(r) -> _ => hit[1] = true,
            send(s, 0) -> _ => hit[2] = true,
            send(s, 0) -> _ => hit[3] = true,
        }
    }
}

#[test]
fn nesting() {
    let (s, r) = channel::unbounded::<i32>();

    select! {
        send(s, 0) -> _ => {
            select! {
                recv(r) -> v => {
                    assert_eq!(v, Ok(0));
                    select! {
                        send(s, 1) -> _ => {
                            select! {
                                recv(r) -> v => {
                                    assert_eq!(v, Ok(1));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
#[should_panic(expected = "send panicked")]
fn panic_send() {
    fn get() -> channel::Sender<i32> {
        panic!("send panicked")
    }

    select! {
        send(get(), panic!()) -> _ => {}
    }
}

#[test]
#[should_panic(expected = "recv panicked")]
fn panic_recv() {
    fn get() -> channel::Receiver<i32> {
        panic!("recv panicked")
    }

    select! {
        recv(get()) -> _ => {}
    }
}

#[test]
fn stress_recv() {
    const COUNT: usize = 10_000;

    let (s1, r1) = channel::unbounded();
    let (s2, r2) = channel::bounded(5);
    let (s3, r3) = channel::bounded(100);

    crossbeam::scope(|scope| {
        scope.spawn(|| {
            for i in 0..COUNT {
                s1.send(i).unwrap();
                r3.recv().unwrap();

                s2.send(i).unwrap();
                r3.recv().unwrap();
            }
        });

        for i in 0..COUNT {
            for _ in 0..2 {
                select! {
                    recv(r1) -> v => assert_eq!(v, Ok(i)),
                    recv(r2) -> v => assert_eq!(v, Ok(i)),
                }

                s3.send(()).unwrap();
            }
        }
    });
}

#[test]
fn stress_send() {
    const COUNT: usize = 10_000;

    let (s1, r1) = channel::bounded(0);
    let (s2, r2) = channel::bounded(0);
    let (s3, r3) = channel::bounded(100);

    crossbeam::scope(|scope| {
        scope.spawn(|| {
            for i in 0..COUNT {
                assert_eq!(r1.recv().unwrap(), i);
                assert_eq!(r2.recv().unwrap(), i);
                r3.recv().unwrap();
            }
        });

        for i in 0..COUNT {
            for _ in 0..2 {
                select! {
                    send(s1, i) -> _ => {},
                    send(s2, i) -> _ => {},
                }
            }
            s3.send(()).unwrap();
        }
    });
}

#[test]
fn stress_mixed() {
    const COUNT: usize = 10_000;

    let (s1, r1) = channel::bounded(0);
    let (s2, r2) = channel::bounded(0);
    let (s3, r3) = channel::bounded(100);

    crossbeam::scope(|scope| {
        scope.spawn(|| {
            for i in 0..COUNT {
                s1.send(i).unwrap();
                assert_eq!(r2.recv().unwrap(), i);
                r3.recv().unwrap();
            }
        });

        for i in 0..COUNT {
            for _ in 0..2 {
                select! {
                    recv(r1) -> v => assert_eq!(v, Ok(i)),
                    send(s2, i) -> _ => {},
                }
            }
            s3.send(()).unwrap();
        }
    });
}

#[test]
fn stress_timeout_two_threads() {
    const COUNT: usize = 20;

    let (s, r) = channel::bounded(2);

    crossbeam::scope(|scope| {
        scope.spawn(|| {
            for i in 0..COUNT {
                if i % 2 == 0 {
                    thread::sleep(ms(500));
                }

                loop {
                    select! {
                        send(s, i) -> _ => break,
                        default(ms(100)) => {}
                    }
                }
            }
        });

        scope.spawn(|| {
            for i in 0..COUNT {
                if i % 2 == 0 {
                    thread::sleep(ms(500));
                }

                loop {
                    select! {
                        recv(r) -> v => {
                            assert_eq!(v, Ok(i));
                            break;
                        }
                        default(ms(100)) => {}
                    }
                }
            }
        });
    });
}

#[test]
fn send_recv_same_channel() {
    let (s, r) = channel::bounded::<i32>(0);
    select! {
        send(s, 0) -> _ => panic!(),
        recv(r) -> _ => panic!(),
        default(ms(500)) => {}
    }

    let (s, r) = channel::unbounded::<i32>();
    select! {
        send(s, 0) -> _ => {},
        recv(r) -> _ => panic!(),
        default(ms(500)) => panic!(),
    }
}

#[test]
fn matching() {
    const THREADS: usize = 44;

    let (s, r) = &channel::bounded::<usize>(0);

    crossbeam::scope(|scope| {
        for i in 0..THREADS {
            scope.spawn(move || {
                select! {
                    recv(r) -> v => assert_ne!(v.unwrap(), i),
                    send(s, i) -> _ => {},
                }
            });
        }
    });

    assert_eq!(r.try_recv(), Err(TryRecvError::Empty));
}

#[test]
fn matching_with_leftover() {
    const THREADS: usize = 55;

    let (s, r) = &channel::bounded::<usize>(0);

    crossbeam::scope(|scope| {
        for i in 0..THREADS {
            scope.spawn(move || {
                select! {
                    recv(r) -> v => assert_ne!(v.unwrap(), i),
                    send(s, i) -> _ => {},
                }
            });
        }
        s.send(!0).unwrap();
    });

    assert_eq!(r.try_recv(), Err(TryRecvError::Empty));
}

#[test]
fn channel_through_channel() {
    const COUNT: usize = 1000;

    type T = Box<Any + Send>;

    for cap in 0..3 {
        let (s, r) = channel::bounded::<T>(cap);

        crossbeam::scope(|scope| {
            scope.spawn(move || {
                let mut s = s;

                for _ in 0..COUNT {
                    let (new_s, new_r) = channel::bounded(cap);
                    let mut new_r: T = Box::new(Some(new_r));

                    select! {
                        send(s, new_r) -> _ => {}
                    }

                    s = new_s;
                }
            });

            scope.spawn(move || {
                let mut r = r;

                for _ in 0..COUNT {
                    r = select! {
                        recv(r) -> mut msg => {
                            msg.unwrap()
                                .downcast_mut::<Option<channel::Receiver<T>>>()
                                .unwrap()
                                .take()
                                .unwrap()
                        }
                    }
                }
            });
        });
    }
}

#[test]
fn linearizable() {
    const COUNT: usize = 100_000;

    for step in 0..2 {
        let (start_s, start_r) = channel::bounded::<()>(0);
        let (end_s, end_r) = channel::bounded::<()>(0);

        let ((s1, r1), (s2, r2)) = if step == 0 {
            (channel::bounded::<i32>(1), channel::bounded::<i32>(1))
        } else {
            (channel::unbounded::<i32>(), channel::unbounded::<i32>())
        };

        crossbeam::scope(|scope| {
            scope.spawn(|| {
                for _ in 0..COUNT {
                    start_s.send(()).unwrap();

                    s1.send(1).unwrap();
                    select! {
                        recv(r1) -> _ => {}
                        recv(r2) -> _ => {}
                        default => unreachable!()
                    }

                    end_s.send(()).unwrap();
                    let _ = r2.try_recv();
                }
            });

            for _ in 0..COUNT {
                start_r.recv().unwrap();

                s2.send(1).unwrap();
                let _ = r1.try_recv();

                end_r.recv().unwrap();
            }
        });
    }
}

#[test]
fn fairness1() {
    const COUNT: usize = 10_000;

    let (s1, r1) = channel::bounded::<()>(COUNT);
    let (s2, r2) = channel::unbounded::<()>();

    for _ in 0..COUNT {
        s1.send(()).unwrap();
        s2.send(()).unwrap();
    }

    let mut hits = [0usize; 4];
    while hits[0] + hits[1] < 2 * COUNT {
        select! {
            recv(r1) -> _ => hits[0] += 1,
            recv(r2) -> _ => hits[1] += 1,
            recv(channel::after(ms(0))) -> _ => hits[2] += 1,
            recv(channel::tick(ms(0))) -> _ => hits[3] += 1,
        }
    }

    assert!(r1.is_empty());
    assert!(r2.is_empty());

    let sum: usize = hits.iter().sum();
    assert!(hits.iter().all(|x| *x >= sum / hits.len() / 2));
}
