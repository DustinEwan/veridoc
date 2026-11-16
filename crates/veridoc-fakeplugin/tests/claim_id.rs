//! The fake plugin must recognise the claim ids the engine sends. Its `claim_id` is a copy
//! of `veridoc_engine::hash::claim_id`; this test is the drift gate between them.

#[test]
fn claim_id_matches_the_engine() {
    for (predicate, _, _) in veridoc_fakeplugin::PROBES {
        assert_eq!(
            veridoc_fakeplugin::claim_id("vd:r:probe", predicate),
            veridoc_engine::hash::claim_id("vd:r:probe", predicate),
            "claim id drifted for predicate {predicate}"
        );
    }
}
