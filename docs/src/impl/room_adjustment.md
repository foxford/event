# Room adjustment

Room adjustment is a process of creating derived [rooms](../api/room.md#room) with shifted
[events](../api/event.md#event) to synchronize them with the video recording.

There are two types of markers that may impact the event position: _segments_ and _stream editiing
events_.

## Segments => original room

The recording contains of multiple segments. Each time the translation interrupts by explictly
stopping it or by network failure or by changing the publisher. There are pauses between these
segments but they are being cut when the recording is being concatenated into a single video file.

So to keep _events_ in sync with video there's a need to collapse those time gaps into a single
point.

One the parameters of the _adjsutment_ operation is a list of _segments_. Each segment is a pair
of timestamps relative to the room opening absolute timestamp. The first element of a pair
is when the video segment started and the seconds is when it finished. Inversion of _segments_
gives gaps.

_Adjustment_ operation doesn't modify anything in the initial _real-time room_. Instead in clones
this _real-time room_ (keeping track to it) and its _events_ with new relative timestamps.
The new _room_ with applied _segments_ is being called the _original room_. The term makes sense
further in context of stream editing.

## Stream editing events => modified room

During the translation a moderator may cut certain parts by creating _stream editing events_:
cut-start and cut-stop. Everything between these two events must be cut from the video during
the transcoding process. So there's another set of gaps to cut.

The process of removing those gaps is the same but this time the _adjustment_ operation derives
a new from from the _original room_ instead of the _real-time room_. So the new so called
_modified room_ applies both _segments_ and _stream editing events_.

Stream editing events gets removed from the _modified room_ because they're already applied.

The reason for having both _original_ and _modified_ rooms instead of just the latter is that
at some point on post-production a moderator may want to reedit the stream by changing _stream 
editing events_ and recreate _modified_ room from the _original_ room once again with different
edits.

## Gap collapsing algorithm

The core algorithm is very simple:

1. _Events_ inside the gap get shifted left to the beginning of the gap.
2. All _events_ after the gap get shifted left by the size of the gap.

Example:

```
 segment1 |xx gap xx| segment2       segment1 + segment2
…---------]--1---2--[---3---4--… => …--------1,2---3---4--…
```

## Corner cases

1. There may be a gap before the first _segment_. _Events_ in this gap get shifted right to the
beginning of the first _segment_.
2. All _events_ after the last _segment_ get shifted to the end of the last _segment_.

## Offset

The recording may contain a preroll: a title in the beginning of the video added on transcoding.
This requires to shift all events right for the preroll duration to keep them in sync.
This duration is being specified with `offset` paramter.

## Room opening difference compensation

There may be a time different between the _room_ opening and actual translation start because
conference and event are different services that may process requests for different times
and those requests from tenants are not synced either.

So there's a `started_at` parameter containing absolute time of translation start.
Subtracting it from the _room_ opening time gives the time difference on which all events
are being shifted to.

## Modified segments

Finally when applying _stream editing events_ _segments_ in the _modified room_ also get changed
because they intersect. So the _adjustment_ operation calculates _modified segments_ that need
to be passed to transcoding to recut the original video according to stream editing events.
