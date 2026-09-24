package io.github.joycx.schemgen.ui;

import net.minecraft.client.gui.widget.SliderWidget;
import net.minecraft.text.Text;

/** A slider over {@code min…max} that snaps to a step and shows its value in its label. */
final class SettingSlider extends SliderWidget {
    /** What the slider edits. */
    interface Binding {
        /** The current value; NaN when there is none. */
        double get();

        void set(double value);

        Text label(double value);
    }

    private final double min;
    private final double max;
    private final double step;
    private final Binding binding;

    SettingSlider(int x, int y, int width, double min, double max, double step, Binding binding) {
        super(x, y, width, 20, Text.empty(), position(binding.get(), min, max));
        this.min = min;
        this.max = max;
        this.step = step;
        this.binding = binding;
        // The value itself, which may lie outside a slider narrower than the field's range.
        setMessage(binding.label(binding.get()));
    }

    private static double position(double value, double min, double max) {
        return Double.isNaN(value) ? 0 : Math.max(0, Math.min(1, (value - min) / (max - min)));
    }

    private double current() {
        double v = min + value * (max - min);
        if (step > 0) {
            v = min + Math.round((v - min) / step) * step;
        }
        return Math.max(min, Math.min(max, v));
    }

    @Override
    protected void updateMessage() {
        setMessage(binding.label(current()));
    }

    @Override
    protected void applyValue() {
        binding.set(current());
    }
}
