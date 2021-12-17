use std::borrow::Borrow;

pub(crate) fn is_false(v: &bool) -> bool {
    !v
}

pub(crate) fn iter_to_csv<I, T>(i: I) -> String
where
    I: IntoIterator<Item = T>,
    T: Borrow<T> + ToString,
{
    i.into_iter()
        .map(|r| r.borrow().to_string())
        .collect::<Vec<_>>()
        .join(",")
}
