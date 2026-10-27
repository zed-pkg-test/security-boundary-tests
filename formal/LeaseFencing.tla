------------------------ MODULE LeaseFencing ------------------------
EXTENDS Naturals, Sequences, TLC

CONSTANTS Writers, MaxEpoch, MaxCommits
ASSUME /\ Writers # {}
       /\ MaxEpoch \in Nat
       /\ MaxCommits \in Nat

VARIABLES epoch, holder, commitHistory
vars == <<epoch, holder, commitHistory>>

Init == /\ epoch = 0
        /\ holder = "none"
        /\ commitHistory = <<>>

Acquire(w) ==
  /\ w \in Writers
  /\ epoch < MaxEpoch
  /\ epoch' = epoch + 1
  /\ holder' = w
  /\ UNCHANGED commitHistory

Commit(w, claimedEpoch) ==
  /\ w \in Writers
  /\ holder = w
  /\ epoch = claimedEpoch
  /\ Len(commitHistory) < MaxCommits
  /\ commitHistory' = Append(commitHistory,
       [writer |-> w, claim |-> claimedEpoch,
        observedEpoch |-> epoch, currentHolder |-> holder])
  /\ UNCHANGED <<epoch, holder>>

Next == \/ \E w \in Writers : Acquire(w)
        \/ \E w \in Writers, e \in 0..MaxEpoch : Commit(w, e)

TypeOK ==
  /\ epoch \in 0..MaxEpoch
  /\ holder \in Writers \cup {"none"}
  /\ commitHistory \in Seq([
      writer : Writers,
      claim : 0..MaxEpoch,
      observedEpoch : 0..MaxEpoch,
      currentHolder : Writers \cup {"none"}])

NoStaleCommit ==
  \A i \in 1..Len(commitHistory) :
    /\ commitHistory[i].claim = commitHistory[i].observedEpoch
    /\ commitHistory[i].writer = commitHistory[i].currentHolder

Spec == Init /\ [][Next]_vars
=============================================================================
