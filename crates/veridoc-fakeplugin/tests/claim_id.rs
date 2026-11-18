//! The fake plugin must recognise the claim ids the engine sends. Its `claim_name` is a copy
//! of `veridoc_engine::hash::claim_name`; this test is the drift gate between them. The left
//! half is the author's subject name, not a referent id (`0009`).

#[test]
fn claim_id_matches_the_engine() {
    for (predicate, _, _) in veridoc_fakeplugin::PROBES {
        assert_eq!(
            veridoc_fakeplugin::claim_name("probe", predicate),
            veridoc_engine::hash::claim_name("probe", predicate),
            "claim id drifted for predicate {predicate}"
        );
    }
}
