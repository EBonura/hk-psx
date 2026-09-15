// Source-only reference input schedule. CSV has the mandatory header
// test_frame,buttons followed by strictly increasing nonnegative decimal frame
// numbers and active-high PS1 masks (decimal or 0x hexadecimal). Blank lines and
// whole-line # comments are allowed; quoted fields and inline comments are not.
//
// Each event replaces the complete held mask from that frame onward. Before the
// first event the mask is zero. The final mask remains held indefinitely: add
// an explicit final `frame,0` event to release; this helper never stops the game.
// An empty schedule (header only) is neutral. Repeated, skipped, or rewound Apply
// calls select the same mask for the requested frame, without hidden cursor state.
// Skipping frames does not replay edges from events that were skipped.
//
// Driver integration: wait for the target scene/Hero to be ready, then Apply(0)
// from LateUpdate. The following native InControl update is test frame 0.
// Apply(n+1) in the next LateUpdate for the next input update, recording the
// device's LastAppliedMask/LastUpdateTick to verify consumption. Setup/teleport
// commands are separate; loading or applying a tape never moves the Hero.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;

namespace HKReference
{
    public sealed class Tape
    {
        private readonly int[] frames;
        private readonly uint[] buttons;

        private Tape(int[] frames, uint[] buttons)
        {
            this.frames = frames;
            this.buttons = buttons;
        }

        public int EventCount { get { return frames.Length; } }
        public int LastEventFrame { get { return frames.Length == 0 ? -1 : frames[frames.Length - 1]; } }

        // Loading is side-effect free with respect to the controller. A bad
        // schedule fails completely instead of partially applying its events.
        public static Tape Load(string path)
        {
            List<int> frames = new List<int>();
            List<uint> buttons = new List<uint>();
            bool header = false;
            int lineNumber = 0;
            using (StreamReader reader = new StreamReader(path))
            {
                string line;
                while ((line = reader.ReadLine()) != null)
                {
                    lineNumber++;
                    line = line.Trim();
                    if (line.Length == 0 || line.StartsWith("#", StringComparison.Ordinal)) continue;
                    string[] columns = line.Split(',');
                    if (columns.Length != 2) throw Invalid(path, lineNumber, "expected two CSV columns");
                    string frameText = columns[0].Trim();
                    string maskText = columns[1].Trim();
                    if (!header)
                    {
                        if (frameText != "test_frame" || maskText != "buttons")
                            throw Invalid(path, lineNumber, "expected test_frame,buttons header");
                        header = true;
                        continue;
                    }

                    int frame;
                    if (!Int32.TryParse(frameText, NumberStyles.None, CultureInfo.InvariantCulture, out frame) || frame < 0)
                        throw Invalid(path, lineNumber, "test_frame must be a nonnegative Int32 decimal integer");
                    if (frames.Count != 0 && frame <= frames[frames.Count - 1])
                        throw Invalid(path, lineNumber, "test_frame must increase strictly; duplicate frames are ambiguous");

                    uint mask;
                    bool hex = maskText.StartsWith("0x", StringComparison.OrdinalIgnoreCase);
                    string digits = hex ? maskText.Substring(2) : maskText;
                    if (!UInt32.TryParse(digits, hex ? NumberStyles.AllowHexSpecifier : NumberStyles.None,
                                         CultureInfo.InvariantCulture, out mask))
                        throw Invalid(path, lineNumber, "buttons must be an unsigned decimal or 0x hexadecimal integer");
                    if ((mask & ~ReplayDevice.SupportedMask) != 0)
                        throw Invalid(path, lineNumber, "buttons includes unsupported PS1 bits");
                    frames.Add(frame);
                    buttons.Add(mask);
                }
            }
            if (!header) throw Invalid(path, lineNumber, "missing test_frame,buttons header");
            return new Tape(frames.ToArray(), buttons.ToArray());
        }

        private static FormatException Invalid(string path, int line, string message)
        {
            return new FormatException(path + ":" + line.ToString(CultureInfo.InvariantCulture) + ": " + message);
        }

        // Pure lookup also lets a validation harness check schedule boundaries
        // without advancing or injecting a native input tick.
        public uint ButtonsAt(int testFrame)
        {
            if (testFrame < 0) throw new ArgumentOutOfRangeException("testFrame");
            int low = 0, high = frames.Length;
            while (low < high)
            {
                int middle = low + (high - low) / 2;
                if (frames[middle] <= testFrame) low = middle + 1;
                else high = middle;
            }
            return low == 0 ? 0 : buttons[low - 1];
        }

        public void Apply(int testFrame)
        {
            ReplayDevice.SetButtons(ButtonsAt(testFrame));
        }
    }
}
