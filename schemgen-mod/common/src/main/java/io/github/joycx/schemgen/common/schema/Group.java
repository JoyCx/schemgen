package io.github.joycx.schemgen.common.schema;

/** A settings section, such as "Size &amp; shape". The schema lists them in display order. */
public record Group(String key, String label, String help) {}
