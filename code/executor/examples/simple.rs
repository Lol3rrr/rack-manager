use executor::{tasks, Runtime, StaticList};

async fn first() {}

async fn second() {}

async fn third() {}

fn main() {
    tasks!(list, (first(), f), (second(), s), (third(), t));

    assert_eq!(3, list.length());

    assert!(list.get(0).is_some());
    assert!(list.get(1).is_some());
    assert!(list.get(2).is_some());
    assert!(list.get(3).is_none());

    let runtime = Runtime::new(list);
    runtime.run();
}
