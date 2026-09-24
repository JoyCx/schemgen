package io.github.joycx.schemgen.common.backend;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.Test;

class SseParserTest {
    private final List<SseParser.Event> events = new ArrayList<>();
    private final SseParser parser = new SseParser(events::add);

    /** What schemgen2 sends for a job: retry first, then one event per state. */
    private static final String STREAM = """
            retry: 3000

            event: progress
            data: {"id":"a","status":"running","progress":27.0}

            : keep-alive

            event: done
            data: {"id":"a","status":"done","progress":100.0}

            """;

    @Test
    void parsesTheServersStream() {
        parser.feed(STREAM);
        assertEquals(2, events.size());
        assertEquals("progress", events.get(0).event());
        assertEquals("{\"id\":\"a\",\"status\":\"running\",\"progress\":27.0}", events.get(0).data());
        assertEquals("done", events.get(1).event());
        assertEquals(3000, parser.retryMillis());
    }

    @Test
    void anySplitOfTheStreamGivesTheSameEvents() {
        parser.feed(STREAM);
        List<SseParser.Event> whole = List.copyOf(events);
        for (int size = 1; size <= 7; size++) {
            events.clear();
            SseParser split = new SseParser(events::add);
            for (int i = 0; i < STREAM.length(); i += size) {
                split.feed(STREAM.substring(i, Math.min(STREAM.length(), i + size)));
            }
            assertEquals(whole, events, "chunks of " + size);
        }
    }

    @Test
    void crlfAndLoneCrEndLinesToo() {
        parser.feed("event: progress\r\ndata: one\r\n\r\nevent: done\rdata: two\r\r");
        assertEquals(List.of(
                new SseParser.Event("progress", "one", ""),
                new SseParser.Event("done", "two", "")), events);
    }

    @Test
    void crlfSplitBetweenChunksIsOneLineBreak() {
        parser.feed("data: x\r");
        parser.feed("\n\r");
        parser.feed("\n");
        assertEquals(List.of(new SseParser.Event("message", "x", "")), events);
    }

    @Test
    void multiLineDataJoinsWithNewlines() {
        parser.feed("data: first\ndata:second\ndata\ndata:  indented\n\n");
        assertEquals("first\nsecond\n\n indented", events.get(0).data());
        assertEquals("message", events.get(0).event());
    }

    @Test
    void commentsAndUnknownFieldsAreIgnored() {
        parser.feed(": hello\nfoo: bar\n:another\n\n");
        assertTrue(events.isEmpty());
        parser.feed("data: after\n\n");
        assertEquals(1, events.size());
    }

    @Test
    void blankLineWithoutDataDispatchesNothingAndResetsTheType() {
        parser.feed("event: done\n\ndata: plain\n\n");
        assertEquals(List.of(new SseParser.Event("message", "plain", "")), events);
    }

    @Test
    void idPersistsAcrossEventsAndRetryMustBeDigits() {
        parser.feed("id: 7\nretry: 1500\ndata: a\n\nretry: soon\ndata: b\n\n");
        assertEquals("7", events.get(0).id());
        assertEquals("7", events.get(1).id());
        assertEquals(1500, parser.retryMillis());
    }

    @Test
    void anUnfinishedEventIsNotDispatched() {
        parser.feed("event: done\ndata: {\"id\":\"a\"}\n");
        assertTrue(events.isEmpty(), "no blank line yet");
        parser.feed("\n");
        assertEquals(1, events.size());
    }

    @Test
    void byteOrderMarkIsSkipped() {
        parser.feed("﻿data: x\n\n");
        assertEquals("x", events.get(0).data());
    }
}
