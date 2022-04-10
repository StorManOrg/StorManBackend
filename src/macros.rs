/// A little macro that makes creating collections (e.g. hashmaps) a bit easier.
///
/// **Example**
/// ```
/// let map: HashMap<&str, &str> = collection!{
///     "foo" => "bar"
/// }
/// ```
#[macro_export]
macro_rules! collection {
    // map-like
    ($($key:expr => $value:expr),* $(,)?) => {{
        core::convert::From::from([$(($key, $value),)*])
    }};

    // set-like
    ($($value:expr),* $(,)?) => {{
        core::convert::From::from([$($value,)*])
    }};
}
