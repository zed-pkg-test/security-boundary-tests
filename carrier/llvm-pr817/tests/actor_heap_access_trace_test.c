#include "oreslang/runtime/ActorOwnedI64Heap.h"
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define REQUIRE(expr) do { \
    if (!(expr)) { \
        fprintf(stderr, "actor heap trace contract failed at line %d: %s\n", __LINE__, #expr); \
        exit(1); \
    } \
} while (0)

static void test_late_activation_is_permanently_closed(void) {
    OresActorI64Heap* heap = ores_actor_i64_heap_open(3003, 9, 1);
    OresActorTurn first = ores_actor_i64_heap_enter(heap, 3003, 9);
    ores_actor_i64_heap_leave(heap, first);
    /* Trace coverage may only start before execution. A host cannot observe
     * a later suffix and present it as complete access evidence. */
    REQUIRE(!ores_actor_i64_heap_trace_enable(heap, 4));
    OresActorHeapAccessSummary summary = {0};
    REQUIRE(!ores_actor_i64_heap_trace_snapshot(heap, NULL, 0, &summary));
    ores_actor_i64_heap_retire(heap);
    REQUIRE(ores_actor_i64_heap_try_destroy(heap));
}

static void test_bounded_successful_accesses(void) {
    OresActorI64Heap* heap = ores_actor_i64_heap_open(1201, 7, 8);
    OresActorHeapAccessSummary summary = {0};
    OresActorHeapAccessEvent records[7];
    memset(records, 0, sizeof(records));
    REQUIRE(!ores_actor_i64_heap_trace_snapshot(heap, records, 7, &summary));
    REQUIRE(!ores_actor_i64_heap_trace_enable(heap, 0));
    REQUIRE(!ores_actor_i64_heap_trace_enable(heap, 65537));
    REQUIRE(ores_actor_i64_heap_trace_enable(heap, 7));
    REQUIRE(!ores_actor_i64_heap_trace_enable(heap, 7));

    OresActorTurn first = ores_actor_i64_heap_enter(heap, 1201, 7);
    REQUIRE(!ores_actor_i64_heap_trace_snapshot(heap, records, 7, &summary));
    REQUIRE(!ores_actor_i64_heap_trace_enable(heap, 2));
    OresActorSlot slot = ores_actor_i64_heap_new(heap, first, 19);
    REQUIRE(ores_actor_i64_heap_read(heap, first, slot) == 19);
    ores_actor_i64_heap_write(heap, first, slot, 20);
    ores_actor_i64_heap_drop(heap, first, slot);
    ores_actor_i64_heap_field_init_mutable(heap, first.domain, first.epoch, 3, 55);
    REQUIRE(ores_actor_i64_heap_field_read(heap, first.domain, first.epoch, 3) == 55);
    ores_actor_i64_heap_field_write(heap, first.domain, first.epoch, 3, 57);
    ores_actor_i64_heap_leave(heap, first);
    REQUIRE(ores_actor_i64_heap_trace_snapshot(heap, records, 7, &summary));
    REQUIRE(summary.enabled && summary.capacity == 7 && summary.stored == 7 && summary.dropped == 0);
    const uint32_t kinds[7] = {
        ORES_ACTOR_HEAP_ACCESS_ALLOC, ORES_ACTOR_HEAP_ACCESS_READ,
        ORES_ACTOR_HEAP_ACCESS_WRITE, ORES_ACTOR_HEAP_ACCESS_DROP,
        ORES_ACTOR_HEAP_ACCESS_FIELD_INIT, ORES_ACTOR_HEAP_ACCESS_FIELD_READ,
        ORES_ACTOR_HEAP_ACCESS_FIELD_WRITE
    };
    for (uint32_t i = 0; i < 7; ++i) {
        REQUIRE(records[i].domain == first.domain);
        REQUIRE(records[i].actor_id == 1201 && records[i].incarnation == 7);
        REQUIRE(records[i].turn_epoch == first.epoch);
        REQUIRE(records[i].kind == kinds[i]);
        REQUIRE(records[i].slot_generation == 1);
        REQUIRE(records[i].slot_index == (i < 4 ? slot.index : 3));
    }
    REQUIRE(!ores_actor_i64_heap_trace_snapshot(heap, records, 6, &summary));
    /* Another turn has a new epoch, and retired slot generations are never
     * laundered into the first incarnation's earlier access identities. */
    OresActorTurn second = ores_actor_i64_heap_enter(heap, 1201, 7);
    REQUIRE(second.epoch != first.epoch);
    REQUIRE(ores_actor_i64_heap_field_read(heap, second.domain, second.epoch, 3) == 57);
    ores_actor_i64_heap_leave(heap, second);
    /* Overflow invalidates certification even though the host still receives
     * diagnostic events and the sticky dropped counter. */
    REQUIRE(!ores_actor_i64_heap_trace_snapshot(heap, records, 7, &summary));
    REQUIRE(summary.stored == 7 && summary.dropped == 1);
    REQUIRE(!ores_actor_i64_heap_trace_snapshot(heap, records, 7, &summary));
    ores_actor_i64_heap_retire(heap);
    REQUIRE(ores_actor_i64_heap_try_destroy(heap));
}

static void test_overflow_and_different_actor_identities(void) {
    OresActorI64Heap* heap = ores_actor_i64_heap_open(9001, 22, 2);
    REQUIRE(ores_actor_i64_heap_trace_enable(heap, 1));
    OresActorTurn turn = ores_actor_i64_heap_enter(heap, 9001, 22);
    OresActorSlot slot = ores_actor_i64_heap_new(heap, turn, 11);
    REQUIRE(ores_actor_i64_heap_read(heap, turn, slot) == 11);
    ores_actor_i64_heap_write(heap, turn, slot, 12);
    ores_actor_i64_heap_leave(heap, turn);
    OresActorHeapAccessEvent event;
    OresActorHeapAccessSummary summary;
    REQUIRE(!ores_actor_i64_heap_trace_snapshot(heap, &event, 1, &summary));
    REQUIRE(summary.stored == 1 && summary.dropped == 2);
    REQUIRE(event.actor_id == 9001 && event.incarnation == 22);
    REQUIRE(event.kind == ORES_ACTOR_HEAP_ACCESS_ALLOC);
    ores_actor_i64_heap_retire(heap);
    REQUIRE(ores_actor_i64_heap_try_destroy(heap));
}

static void test_exact_snapshots_generation_reuse_and_leases(void) {
    OresActorI64Heap* heap = ores_actor_i64_heap_open(2002, 5, 1);
    OresActorHeapAccessSummary summary = {0};
    REQUIRE(ores_actor_i64_heap_trace_enable(heap, 8));
    /* Zero recorded events can be snapshotted with a zero-capacity output. */
    REQUIRE(ores_actor_i64_heap_trace_snapshot(heap, NULL, 0, &summary));
    REQUIRE(summary.stored == 0 && summary.dropped == 0);
    OresActorTurn first = ores_actor_i64_heap_enter(heap, 2002, 5);
    OresActorSlot original = ores_actor_i64_heap_new(heap, first, 9);
    OresActorLoan loan = ores_actor_i64_heap_borrow(heap, first, original);
    REQUIRE(ores_actor_i64_heap_read(heap, first, original) == 9);
    ores_actor_i64_heap_leave(heap, first);
    /* Shared read loans are permitted to cross turn boundaries, and the
     * actor cannot be reclaimed before the final loan release. */
    ores_actor_i64_heap_release(heap, loan);
    OresActorTurn second = ores_actor_i64_heap_enter(heap, 2002, 5);
    ores_actor_i64_heap_drop(heap, second, original);
    OresActorSlot reused = ores_actor_i64_heap_new(heap, second, 10);
    REQUIRE(reused.index == original.index);
    REQUIRE(reused.generation == original.generation + 1);
    REQUIRE(ores_actor_i64_heap_read(heap, second, reused) == 10);
    ores_actor_i64_heap_leave(heap, second);
    OresActorHeapAccessEvent events[8] = {0};
    OresActorHeapAccessSummary sentinel = {99, 99, 99, 99};
    REQUIRE(!ores_actor_i64_heap_trace_snapshot(heap, events, 4, &sentinel));
    REQUIRE(sentinel.dropped == 99 && sentinel.stored == 99);
    REQUIRE(!ores_actor_i64_heap_trace_snapshot(heap, events, 8, NULL));
    REQUIRE(ores_actor_i64_heap_trace_snapshot(heap, events, 8, &summary));
    REQUIRE(summary.stored == 5 && summary.dropped == 0);
    REQUIRE(events[0].kind == ORES_ACTOR_HEAP_ACCESS_ALLOC);
    REQUIRE(events[1].kind == ORES_ACTOR_HEAP_ACCESS_READ);
    REQUIRE(events[2].kind == ORES_ACTOR_HEAP_ACCESS_DROP);
    REQUIRE(events[3].kind == ORES_ACTOR_HEAP_ACCESS_ALLOC);
    REQUIRE(events[4].kind == ORES_ACTOR_HEAP_ACCESS_READ);
    REQUIRE(events[0].slot_generation == original.generation);
    REQUIRE(events[3].slot_generation == reused.generation);
    REQUIRE(events[0].turn_epoch == first.epoch);
    REQUIRE(events[4].turn_epoch == second.epoch);
    ores_actor_i64_heap_retire(heap);
    REQUIRE(ores_actor_i64_heap_try_destroy(heap));
}

int main(void) {
    test_late_activation_is_permanently_closed();
    test_exact_snapshots_generation_reuse_and_leases();
    test_bounded_successful_accesses();
    test_overflow_and_different_actor_identities();
    puts("ACTOR_HEAP_ACCESS_EVIDENCE|scope=native_i64_host_only|result=PASS");
    return 0;
}
