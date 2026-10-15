// Event-level PlayMaker trace for the fidelity gates (docs/FSM-V2-DESIGN.md section 4.1).
// The hooks are called from the clone's PlayMaker.dll (patcher/Program.cs inserts the
// calls): state entry, event receipt, transition, and per action enter, tick and finish.
// Only FSMs on a GameObject under a root named by HK_REFERENCE_FSMTRACE (comma separated) are written.
// Read-only: nothing here changes what the FSMs do.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
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

        public static void Initialize(string output)
        {
            string spec = Environment.GetEnvironmentVariable("HK_REFERENCE_FSMTRACE");
            if (String.IsNullOrEmpty(spec)) return;
            roots = spec.Split(',');
            writer = new StreamWriter(System.IO.Path.Combine(output, "fsmtrace.csv"), false);
            writer.WriteLine("test_frame,physics_steps,unity_frame,kind,fsm,state,a,b,c");
        }

        // A forced-entry stimulus (design section 4.2): at the given test frame, send one named
        // event to one named FSM, so a state the tape cannot reach is still entered with the
        // original as the oracle. HK_REFERENCE_FSMSEND = "GameObject|FSM|EVENT|test frame".
        private static string[] send;
        private static bool sent;

        public static void Poll(int frame)
        {
            if (writer == null) return;
            if (send == null && !sent)
            {
                string spec = Environment.GetEnvironmentVariable("HK_REFERENCE_FSMSEND");
                if (String.IsNullOrEmpty(spec)) { sent = true; return; }
                send = spec.Split('|');
            }
            if (sent || send == null || frame < Int32.Parse(send[3], CultureInfo.InvariantCulture)) return;
            GameObject go = GameObject.Find(send[0]);
            if (go == null) return;
            foreach (PlayMakerFSM f in go.GetComponents<PlayMakerFSM>())
            {
                if (f.FsmName != send[1]) continue;
                sent = true;
                writer.WriteLine(string.Join(",", new string[] { frame.ToString(CultureInfo.InvariantCulture), Steps.ToString(CultureInfo.InvariantCulture),
                    Time.frameCount.ToString(CultureInfo.InvariantCulture), "FORCED_EVENT", Q(send[0] + "/" + send[1]), Q(f.ActiveStateName), Q(send[2]), "", "" }));
                f.SendEvent(send[2]);
                return;
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
            for (Transform t = go == null ? null : go.transform; t != null && !hit; t = t.parent) hit = Array.IndexOf(roots, t.name) >= 0;
            if (hit) tracked[key] = true;
            return hit;
        }

        private static string Id(Fsm fsm)
        {
            GameObject go = fsm.GameObject;
            return (go == null ? "?" : go.name + "#" + go.GetInstanceID().ToString(CultureInfo.InvariantCulture)) + "/" + fsm.Name;
        }

        private static string Q(string s) { return "\"" + (s ?? "").Replace("\"", "'") + "\""; }

        private static void Row(string kind, Fsm fsm, string state, string a, string b, string c)
        {
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
                if (Tracked(fsm)) Row("EVENT", fsm, fsm.ActiveStateName, ev == null ? "" : ev.Name, "", "");
            }
            catch (Exception) { }
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
