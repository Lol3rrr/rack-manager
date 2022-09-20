use executor::{tasks, Runtime, StaticList, TaskList};

async fn first() {}

async fn second() {}

async fn third() {}

fn main() {
    tasks!(list, (first(), f), (second(), s), (third(), t));

    assert_eq!(3, list.length());

    assert!(list.get_task(0).is_some());
    assert!(list.get_task(1).is_some());
    assert!(list.get_task(2).is_some());
    assert!(list.get_task(3).is_none());

    let runtime = Runtime::new(list);
    runtime.run();
}
