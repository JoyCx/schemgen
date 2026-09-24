package io.github.joycx.schemgen.common.schema;

/** A file format the server writes: a value of the {@code format} setting, its label and its file extension. */
public record Format(String id, String label, String extension) {}
