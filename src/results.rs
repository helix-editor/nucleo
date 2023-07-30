pub struct MatchSnapshot {
    chunks: Vec<Match>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy)]
struct Match {
    score: u32,
    idx: u32,
}
