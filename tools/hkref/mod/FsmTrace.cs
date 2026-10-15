// Event-level PlayMaker trace for the fidelity gates (docs/FSM-V2-DESIGN.md section 4.1).
// The hooks are called from the clone's PlayMaker.dll (patcher/Program.cs inserts the
// calls): state entry, event receipt, transition, and per action enter, tick and finish.
// Only FSMs on a GameObject under a root named by HK_REFERENCE_FSMTRACE (comma separated) are written.
// Read-only: nothing here changes what the FSMs do.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.Diagnostics;
using System.IO;
using System.Text;
using HutongGames.PlayMaker;
using UnityEngine;

namespace HKReference
{
    public static class FsmTrace
    {
        private static StreamWriter writer;
        private static string[] roots;
        private static readonly Dictionary<int, bool> tracked = new Dictionary<int, bool>();
        /// <summary>Observed test frame and fixed-step count, set by the Driver.</summary>
        public static int Frame = -1;
        public static int Steps;
        private static int rows;
        private static readonly List<GameObject> rootObjects = new List<GameObject>();
        private static readonly Dictionary<int, Dictionary<string, string>> lastVars = new Dictionary<int, Dictionary<string, string>>();

        public static void Initialize(string output)
        {
            string spec = Environment.GetEnvironmentVariable("HK_REFERENCE_FSMTRACE");
            if (String.IsNullOrEmpty(spec)) return;
            roots = spec.Split(',');
            writer = new StreamWriter(System.IO.Path.Combine(output, "fsmtrace.csv"), false);
            writer.WriteLine("test_frame,physics_steps,unity_frame,kind,fsm,state,a,b,c");
        }

        // Hero stimulus for a state the tape cannot reach: HK_REFERENCE_HEROSEEK = "Root|dx|dy|frame;...": at that
        // test frame the knight is placed at the named root's position plus (dx, dy).
        private static string[][] seeks;
        private static bool[] seeked;

        private static void Seek(int frame)
        {
            if (seeks == null)
            {
                string spec = Environment.GetEnvironmentVariable("HK_REFERENCE_HEROSEEK");
                string[] items = String.IsNullOrEmpty(spec) ? new string[0] : spec.Split(';');
                seeks = new string[items.Length][];
                for (int i = 0; i < items.Length; i++) seeks[i] = items[i].Split('|');
                seeked = new bool[items.Length];
            }
            for (int i = 0; i < seeks.Length; i++)
            {
                if (seeked[i] || frame < Int32.Parse(seeks[i][3], CultureInfo.InvariantCulture)) continue;
                GameObject root = GameObject.Find(seeks[i][0]);
                GameObject hero = GameObject.FindWithTag("Player");
                if (root == null || hero == null) continue;
                Vector3 p = root.transform.position;
                float dx = Single.Parse(seeks[i][1], CultureInfo.InvariantCulture), dy = Single.Parse(seeks[i][2], CultureInfo.InvariantCulture);
                hero.transform.position = new Vector3(p.x + dx, p.y + dy, hero.transform.position.z);
                Rigidbody2D rb = hero.GetComponent<Rigidbody2D>();
                if (rb != null) rb.velocity = Vector2.zero;
                seeked[i] = true;
                writer.WriteLine(string.Join(",", new string[] { frame.ToString(CultureInfo.InvariantCulture), Steps.ToString(CultureInfo.InvariantCulture),
                    Time.frameCount.ToString(CultureInfo.InvariantCulture), "HERO_SEEK", Q(seeks[i][0]), Q(""), Q(dx.ToString(CultureInfo.InvariantCulture)), Q(dy.ToString(CultureInfo.InvariantCulture)), "" }));
            }
        }

        // One ENV row per test frame: the knight, the camera and every tracked root (position, velocity), the
        // numbers the environment-reading actions saw. Replayed by the host harness.
        private static void Env(int frame)
        {
            if (rootObjects.Count == 0 || Time.frameCount == envFrame) return;
            envFrame = Time.frameCount;
            GameObject hero = GameObject.FindWithTag("Player");
            Camera cam = Camera.main;
            StringBuilder b = new StringBuilder();
            Vector3 h = hero == null ? Vector3.zero : hero.transform.position;
            Vector3 c = cam == null ? Vector3.zero : cam.transform.position;
            b.Append("rt ").Append(Time.realtimeSinceStartup.ToString("R", CultureInfo.InvariantCulture)).Append(";time ").Append(Time.time.ToString("R", CultureInfo.InvariantCulture));
            b.Append(";hero ").Append(Show(h)).Append(";cam ").Append(Show(c));
            foreach (GameObject root in rootObjects)
            {
                if (root == null) continue;
                Rigidbody2D rb = root.GetComponent<Rigidbody2D>();
                b.Append(';').Append(root.name).Append('#').Append(root.GetInstanceID().ToString(CultureInfo.InvariantCulture)).Append('@')
                    .Append(Show(root.transform.position)).Append(' ').Append(rb == null ? "0 0" : Show(rb.velocity));
            }
            writer.WriteLine(string.Join(",", new string[] { frame.ToString(CultureInfo.InvariantCulture), Steps.ToString(CultureInfo.InvariantCulture),
                Time.frameCount.ToString(CultureInfo.InvariantCulture), "ENV", Q(""), Q(""), Q(b.ToString()), "", "" }));
        }

        public static void Poll(int frame)
        {
            if (writer == null) return;
            Seek(frame);
            Env(frame);
            Forced(frame);
        }

        // A forced-entry stimulus (design section 4.2): at the given test frame, send one named event to one
        // named FSM, so a state the tape cannot reach is still entered with the original as the oracle.
        // HK_REFERENCE_FSMSEND = "GameObject|FSM|EVENT|test frame;..." (any number, ';' separated).
        private static string[][] sends;
        private static bool[] done;

        private static void Forced(int frame)
        {
            if (sends == null)
            {
                string spec = Environment.GetEnvironmentVariable("HK_REFERENCE_FSMSEND");
                string[] items = String.IsNullOrEmpty(spec) ? new string[0] : spec.Split(';');
                sends = new string[items.Length][];
                for (int i = 0; i < items.Length; i++) sends[i] = items[i].Split('|');
                done = new bool[items.Length];
            }
            for (int i = 0; i < sends.Length; i++)
            {
                if (done[i] || frame < Int32.Parse(sends[i][3], CultureInfo.InvariantCulture)) continue;
                GameObject go = GameObject.Find(sends[i][0]);
                if (go == null) continue;
                foreach (PlayMakerFSM f in go.GetComponents<PlayMakerFSM>())
                {
                    if (f.FsmName != sends[i][1]) continue;
                    done[i] = true;
                    writer.WriteLine(string.Join(",", new string[] { frame.ToString(CultureInfo.InvariantCulture), Steps.ToString(CultureInfo.InvariantCulture),
                        Time.frameCount.ToString(CultureInfo.InvariantCulture), "FORCED_EVENT", Q(sends[i][0] + "/" + sends[i][1]), Q(f.ActiveStateName), Q(sends[i][2]), "", "" }));
                    f.SendEvent(sends[i][2]);
                    break;
                }
            }
        }

        public static void Dispose() { if (writer != null) { writer.Flush(); writer.Dispose(); writer = null; } }

        private static bool Tracked(Fsm fsm)
        {
            if (writer == null || fsm == null) return false;
            int key = fsm.GetHashCode();
            bool hit;
            if (tracked.TryGetValue(key, out hit) && hit) return true;
            // A miss is not cached: an enemy is instantiated first and parented under its
            // root afterwards, so an FSM can fail the check on its first callbacks.
            GameObject go = fsm.GameObject;
            Transform root = null;
            for (Transform t = go == null ? null : go.transform; t != null && !hit; t = t.parent) { hit = Array.IndexOf(roots, t.name) >= 0; if (hit) root = t; }
            if (hit) { tracked[key] = true; if (!rootObjects.Contains(root.gameObject)) rootObjects.Add(root.gameObject); }
            return hit;
        }

        private static string Id(Fsm fsm)
        {
            GameObject go = fsm.GameObject;
            return (go == null ? "?" : go.name + "#" + go.GetInstanceID().ToString(CultureInfo.InvariantCulture)) + "/" + fsm.Name;
        }

        private static string Q(string s) { return "\"" + (s ?? "").Replace("\"", "'") + "\""; }

        private static string Show(object v)
        {
            if (v is float) return ((float)v).ToString("R", CultureInfo.InvariantCulture);
            if (v is Vector2) { Vector2 w = (Vector2)v; return w.x.ToString("R", CultureInfo.InvariantCulture) + " " + w.y.ToString("R", CultureInfo.InvariantCulture); }
            if (v is Vector3) { Vector3 w = (Vector3)v; return w.x.ToString("R", CultureInfo.InvariantCulture) + " " + w.y.ToString("R", CultureInfo.InvariantCulture) + " " + w.z.ToString("R", CultureInfo.InvariantCulture); }
            return Convert.ToString(v, CultureInfo.InvariantCulture);
        }

        // Every variable of the FSM as name -> text: what the environment-reading actions stored.
        private static Dictionary<string, string> Snapshot(Fsm fsm)
        {
            Dictionary<string, string> d = new Dictionary<string, string>();
            FsmVariables v = fsm.Variables;
            foreach (FsmFloat x in v.FloatVariables) d[x.Name] = Show(x.Value);
            foreach (FsmInt x in v.IntVariables) d[x.Name] = Show(x.Value);
            foreach (FsmBool x in v.BoolVariables) d[x.Name] = x.Value ? "true" : "false";
            foreach (FsmString x in v.StringVariables) d[x.Name] = x.Value ?? "";
            foreach (FsmVector2 x in v.Vector2Variables) d[x.Name] = Show(x.Value);
            foreach (FsmVector3 x in v.Vector3Variables) d[x.Name] = Show(x.Value);
            foreach (FsmGameObject x in v.GameObjectVariables) d[x.Name] = x.Value == null ? "null" : x.Value.name;
            return d;
        }

        private static void Vars(Fsm fsm, string state)
        {
            int key = fsm.GetHashCode();
            Dictionary<string, string> now = Snapshot(fsm), before;
            lastVars.TryGetValue(key, out before);
            StringBuilder diff = new StringBuilder();
            foreach (KeyValuePair<string, string> kv in now)
            {
                string old;
                if (before != null && before.TryGetValue(kv.Key, out old) && old == kv.Value) continue;
                if (diff.Length > 0) diff.Append(';');
                diff.Append(kv.Key).Append('=').Append(kv.Value);
            }
            lastVars[key] = now;
            if (diff.Length == 0) return;
            writer.WriteLine(string.Join(",", new string[] {
                Frame.ToString(CultureInfo.InvariantCulture), Steps.ToString(CultureInfo.InvariantCulture),
                Time.frameCount.ToString(CultureInfo.InvariantCulture), "VARS", Q(Id(fsm)), Q(state), Q(diff.ToString()), "", "" }));
        }

        private static int envFrame = -1;

        private static void Row(string kind, Fsm fsm, string state, string a, string b, string c)
        {
            // One ENV row per Unity frame that has any FSM row, so the replay sees the loading frames too.
            if (Time.frameCount != envFrame) Env(Frame);
            Vars(fsm, state);
            writer.WriteLine(string.Join(",", new string[] {
                Frame.ToString(CultureInfo.InvariantCulture), Steps.ToString(CultureInfo.InvariantCulture),
                Time.frameCount.ToString(CultureInfo.InvariantCulture), kind, Q(Id(fsm)), Q(state), Q(a), Q(b), Q(c) }));
            if (++rows % 500 == 0) writer.Flush();
        }

        // ---- hooks, called from PlayMaker.dll ----
        // The parameters are `object` so the call sites the patcher writes into
        // PlayMaker.dll need no PlayMaker type in a signature that points back at it.

        public static void StateEnter(object fsmObject, object stateObject)
        {
            try
            {
                Fsm fsm = (Fsm)fsmObject;
                FsmState state = (FsmState)stateObject;
                if (Tracked(fsm)) Row("STATE_ENTER", fsm, state == null ? "" : state.Name, fsm.ActiveStateName, "", "");
            }
            catch (Exception) { }
        }

        public static void EventReceived(object fsmObject, object eventObject)
        {
            try
            {
                Fsm fsm = (Fsm)fsmObject;
                FsmEvent ev = (FsmEvent)eventObject;
                if (Tracked(fsm)) Row("EVENT", fsm, fsm.ActiveStateName, ev == null ? "" : ev.Name, Cause(), "");
            }
            catch (Exception) { }
        }

        // What raised the event now being processed: the PlayMaker action running (its type and method), a
        // Finish, or the outside code that called into PlayMaker (its first non-PlayMaker frame).
        private static string Cause()
        {
            StackFrame[] frames = new StackTrace(2, false).GetFrames();
            if (frames == null) return "?";
            string outside = null;
            foreach (StackFrame f in frames)
            {
                System.Reflection.MethodBase m = f.GetMethod();
                if (m == null || m.DeclaringType == null) continue;
                Type t = m.DeclaringType;
                if (typeof(FsmStateAction).IsAssignableFrom(t))
                {
                    if (t == typeof(FsmStateAction)) { if (m.Name == "Finish") return "finish"; continue; }
                    return "action:" + t.Name + "." + m.Name;
                }
                if (t.Namespace == "HutongGames.PlayMaker" && (t.Name == "Fsm" || t.Name == "FsmState" || t.Name == "FsmEvent")) continue;
                if (outside == null && !(t.Namespace ?? "").StartsWith("System") && !(t.Namespace ?? "").StartsWith("UnityEngine") && t.Name != "PlayMakerFSM") outside = t.Name + "." + m.Name;
                if (outside == null && t.Name == "PlayMakerFSM") outside = "PlayMakerFSM." + m.Name;
            }
            return "ext:" + (outside ?? "?");
        }

        public static void Transition(object fsmObject, object transitionObject)
        {
            try
            {
                Fsm fsm = (Fsm)fsmObject;
                FsmTransition t = (FsmTransition)transitionObject;
                if (Tracked(fsm)) Row("TRANSITION", fsm, fsm.ActiveStateName, t.EventName, t.ToState, "");
            }
            catch (Exception) { }
        }

        public static void ActionEnter(object action, int index)
        {
            try { Action("ACTION_ENTER", (FsmStateAction)action, index.ToString(CultureInfo.InvariantCulture)); } catch (Exception) { }
        }

        public static void ActionTick(object action, int phase)
        {
            try { Action("ACTION_TICK", (FsmStateAction)action, phase == 0 ? "update" : phase == 1 ? "fixed" : "late"); } catch (Exception) { }
        }

        public static void ActionFinish(object action)
        {
            try { Action("ACTION_FINISH", (FsmStateAction)action, ""); } catch (Exception) { }
        }

        private static void Action(string kind, FsmStateAction action, string detail)
        {
            Fsm fsm = action == null ? null : action.Fsm;
            if (!Tracked(fsm)) return;
            FsmState s = action.State;
            Row(kind, fsm, s == null ? "" : s.Name, action.GetType().Name, detail, "");
        }
    }
}
