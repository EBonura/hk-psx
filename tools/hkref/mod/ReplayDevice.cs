// Reference-driver input for the user's isolated Windows game copy. The
// installed Assembly-CSharp.dll contains these InControl APIs; no OS input,
// HeroController methods, physics, or action edge flags are replaced here.
using InControl;
using System.IO;
using System.Globalization;
using UnityEngine;

namespace HKReference
{
    public sealed class ReplayDevice : InputDevice
    {
        // Active-high original PS1 button bits, matching the port's tape input.
        public const uint Start = 0x0008;
        public const uint Up = 0x0010;
        public const uint Right = 0x0020;
        public const uint Down = 0x0040;
        public const uint Left = 0x0080;
        public const uint Focus = 0x2000; // Circle
        public const uint Jump = 0x4000; // Cross
        public const uint Attack = 0x8000; // Square
        public const uint SupportedMask = Start | Up | Right | Down | Left | Focus | Jump | Attack;

        private static ReplayDevice device;
        private static HeroActions boundActions;
        private static uint requestedMask;
        private static StreamWriter inputLog;
        public static int TestFrame { get; set; } = -1;
        public static void OpenLog(string output)
        {
            inputLog = new StreamWriter(Path.Combine(output, "input-events.csv"));
            inputLog.AutoFlush = true;
            inputLog.WriteLine("test_frame,unity_frame,time,input_tick,buttons");
        }
        public static void CloseLog() { if (inputLog != null) { inputLog.Dispose(); inputLog = null; } }

        public static uint RequestedMask { get { return requestedMask; } }
        public static uint LastAppliedMask { get; private set; }
        public static ulong LastUpdateTick { get; private set; }
        public static ulong UpdateCount { get; private set; }

        // InputHandler.ControllerAttached/ControllerActivated remap all known
        // pads to a platform layout, overwriting our explicit bindings on the
        // first press. Their verified unknown-device guard leaves this device
        // alone. DeviceBindingSource reads its controls directly in either case.
        public override bool IsKnown { get { return false; } }

        private ReplayDevice() : base("HK reference replay")
        {
            AddControl(InputControlType.DPadUp, "Up");
            AddControl(InputControlType.DPadRight, "Right");
            AddControl(InputControlType.DPadDown, "Down");
            AddControl(InputControlType.DPadLeft, "Left");
            AddControl(InputControlType.Action1, "Jump");
            AddControl(InputControlType.Action2, "Attack");
            AddControl(InputControlType.Action3, "Focus");
            AddControl(InputControlType.Start, "Pause");
        }

        // Safe to poll from the persistent driver while managers/scenes load.
        // Scene initialization can replace the action set; bind that new set
        // once rather than clearing input history on every frame.
        public static bool TryAttach()
        {
            if (!InputManager.IsSetup || InputHandler.Instance == null ||
                InputHandler.Instance.inputActions == null)
                return false;
            Attach(InputHandler.Instance.inputActions);
            return true;
        }

        public static void Attach(HeroActions actions)
        {
            if (device == null)
            {
                device = new ReplayDevice();
                InputManager.AttachDevice(device);
            }
            InputManager.SuspendInBackground = false;
            if (object.ReferenceEquals(boundActions, actions))
                return;

            // Remove all keyboard/real-controller bindings, including unused
            // actions, so the reference has one deterministic input source.
            foreach (PlayerAction action in actions.Actions)
                action.ClearBindings();
            actions.ClearInputState();
            actions.Device = device;
            Bind(actions.up, InputControlType.DPadUp);
            Bind(actions.right, InputControlType.DPadRight);
            Bind(actions.down, InputControlType.DPadDown);
            Bind(actions.left, InputControlType.DPadLeft);
            Bind(actions.jump, InputControlType.Action1);
            Bind(actions.attack, InputControlType.Action2);
            // The original ListenForCast FSM reads cast for both SOUL healing and spells.
            Bind(actions.cast, InputControlType.Action3);
            Bind(actions.pause, InputControlType.Start);
            boundActions = actions;
        }

        private static void Bind(PlayerAction action, InputControlType control)
        {
            action.AddBinding(new DeviceBindingSource(control));
        }

        // Consumed on the next real InControl update. Send 0 for neutral;
        // holding Jump for several updates and releasing it generates the
        // genuine WasPressed/IsPressed/WasReleased sequence through Commit.
        public static void SetButtons(uint mask)
        {
            requestedMask = mask & SupportedMask;
        }

        public override void Update(ulong updateTick, float deltaTime)
        {
            uint mask = requestedMask;
            if (inputLog != null) inputLog.WriteLine(string.Join(",", TestFrame.ToString(CultureInfo.InvariantCulture),
                Time.frameCount.ToString(CultureInfo.InvariantCulture), Time.time.ToString(CultureInfo.InvariantCulture),
                updateTick.ToString(CultureInfo.InvariantCulture), mask.ToString(CultureInfo.InvariantCulture)));
            UpdateWithState(InputControlType.DPadUp, (mask & Up) != 0, updateTick, deltaTime);
            UpdateWithState(InputControlType.DPadRight, (mask & Right) != 0, updateTick, deltaTime);
            UpdateWithState(InputControlType.DPadDown, (mask & Down) != 0, updateTick, deltaTime);
            UpdateWithState(InputControlType.DPadLeft, (mask & Left) != 0, updateTick, deltaTime);
            UpdateWithState(InputControlType.Action1, (mask & Jump) != 0, updateTick, deltaTime);
            UpdateWithState(InputControlType.Action2, (mask & Attack) != 0, updateTick, deltaTime);
            UpdateWithState(InputControlType.Action3, (mask & Focus) != 0, updateTick, deltaTime);
            UpdateWithState(InputControlType.Start, (mask & Start) != 0, updateTick, deltaTime);
            LastAppliedMask = mask;
            LastUpdateTick = updateTick;
            UpdateCount++;
            // InputManager next calls InputDevice.Commit, then updates action
            // sets and combined movement vectors. Do not commit a second time.
        }
    }
}
