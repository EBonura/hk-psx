// Closed-loop test hero for audio and behaviour traces of the original. Plays through the
// same virtual device a tape does, so the game's own logic produces everything recorded.
// Not a bot for fairness: it walks, jumps when blocked, swings at what is in reach and
// refills health (HK_REFERENCE_AUTOPILOT_HEAL=1) so a long fight is not cut short.
//
// HK_REFERENCE_AUTOPILOT = "fight[:Name]"   go to and hit the nearest live enemy whose name
//                                           contains Name (any enemy when omitted)
//                          "fight:Name@X"   as fight, and walk toward world x = X while no such enemy is alive
//                          HK_REFERENCE_AUTOPILOT_ENEMY_HP=n  matching enemies start with n hit points
//                          "walk:X[:Name]"  walk to world x = X, swinging at enemies in reach
//                                           (Name restricts the swing targets); ends at the goal or on a scene change
//                          "talk[:N]"       walk to each NPC (a "Conversation Control" FSM) in the scene in turn, nearest first,
//                                           and press Up and Jump until its lines run out or 900 frames pass; N NPCs (default 4)
//                          "route:g1,g2,.." in each scene walk to the transition gate named gN, swinging at what is
//                                           in reach, through the gate into the next scene; ends after the last gate.
//                                           A scene that holds the hero longer than HK_REFERENCE_AUTOPILOT_SCENE_FRAMES
//                                           (default 2400) is left by a direct load through the same gate (logged).
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using UnityEngine;

namespace HKReference
{
    public sealed class Autopilot
    {
        private readonly bool fight, route, talk;
        private readonly int talkCount = 4;
        private readonly HashSet<int> talked = new HashSet<int>();
        private Component npc;
        private int npcSince, npcArrive;
        private float goalX;
        private readonly bool hasFallback;
        private readonly string filter;
        private readonly bool heal;
        private readonly string[] gates;
        private readonly int sceneBudget;
        private readonly string log;
        private readonly System.Random rng = new System.Random(7);
        private readonly List<HealthManager> actors = new List<HealthManager>();
        private string startScene;
        private int calls, jumpHold, attackHold, lastTurn, stillFrames, goalFrames, routeIndex, sceneStart, wander, wanderDir;
        private int bestAt;
        private float lastX = float.NaN, best = float.MaxValue;
        private Component gate;
        private readonly int bossHp;
        private readonly HashSet<int> trimmed = new HashSet<int>();
        private bool reached;
        private string expectScene = "", expectEntry = "";
        public bool Done { get; private set; }
        public string Why = "";

        public static Autopilot FromEnvironment(string output)
        {
            string spec = Environment.GetEnvironmentVariable("HK_REFERENCE_AUTOPILOT");
            if (String.IsNullOrEmpty(spec)) return null;
            return new Autopilot(spec, output);
        }

        private Autopilot(string spec, string output)
        {
            log = Path.Combine(output, "autopilot.log");
            int colon = spec.IndexOf(':');
            string kind = colon < 0 ? spec : spec.Substring(0, colon), rest = colon < 0 ? "" : spec.Substring(colon + 1);
            fight = kind == "fight";
            route = kind == "route";
            talk = kind == "talk";
            if (talk && rest.Length > 0) Int32.TryParse(rest, out talkCount);
            if (!fight && !route && !talk && kind != "walk") throw new ArgumentException("HK_REFERENCE_AUTOPILOT: " + spec);
            sceneBudget = 2400;
            Int32.TryParse(Environment.GetEnvironmentVariable("HK_REFERENCE_AUTOPILOT_SCENE_FRAMES"), out sceneBudget);
            if (sceneBudget <= 0) sceneBudget = 2400;
            if (route) gates = rest.Split(',');
            else if (kind == "walk")
            {
                string[] p = rest.Split(':');
                goalX = Single.Parse(p[0], CultureInfo.InvariantCulture);
                filter = p.Length > 1 && p[1].Length > 0 ? p[1] : null;
            }
            else
            {
                filter = rest.Length > 0 ? rest : null;
                if (filter != null && filter.Contains("@"))
                {
                    string[] f = filter.Split('@');
                    filter = f[0].Length > 0 ? f[0] : null;
                    goalX = Single.Parse(f[1], CultureInfo.InvariantCulture);
                    hasFallback = true;
                }
            }
            heal = Environment.GetEnvironmentVariable("HK_REFERENCE_AUTOPILOT_HEAL") == "1";
            Int32.TryParse(Environment.GetEnvironmentVariable("HK_REFERENCE_AUTOPILOT_ENEMY_HP"), out bossHp);
        }

        // Name filter: substrings separated by '|', any of them.
        private bool Matches(string name)
        {
            foreach (string part in filter.Split('|'))
                if (part.Length > 0 && name.IndexOf(part, StringComparison.Ordinal) >= 0) return true;
            return false;
        }

        private void Note(string text) { File.AppendAllText(log, "frame=" + calls + " " + text + Environment.NewLine); }

        private void Finish(string why) { Done = true; Why = why; ReplayDevice.SetButtons(0); }

        public void Step(int frame, object gm, Component hero)
        {
            if (hero == null || Done) return;
            string scene = UnityEngine.SceneManagement.SceneManager.GetActiveScene().name;
            if (startScene == null) { startScene = scene; sceneStart = calls; Note("start " + scene); }
            if (scene != startScene)
            {
                if (!route) { Finish("left " + startScene + " for " + scene); return; }
                if (expectScene.Length > 0 && scene != expectScene)
                {
                    // Fell or was carried off the route: put the Knight back where the gate would have.
                    Note("off route in " + scene + " (expected " + expectScene + "), direct load via " + expectEntry);
                    startScene = scene;
                    Driver.Write(gm, "entryGateName", expectEntry);
                    ReplayDevice.SetButtons(0);
                    string to = expectScene;
                    expectScene = "";
                    Driver.Call(gm, "LoadScene", to);
                    return;
                }
                routeIndex++;
                Note("entered " + scene + " after " + (calls - sceneStart) + " frames in " + startScene);
                startScene = scene; sceneStart = calls; gate = null; best = float.MaxValue; wander = 0; stillFrames = 0; lastX = float.NaN;
                if (routeIndex >= gates.Length) { Finish("route complete"); return; }
            }
            calls++;
            if (heal)
            {
                object pd = Driver.Read(gm, "playerData");
                int hp = Convert.ToInt32(Driver.Read(pd, "health")), max = Convert.ToInt32(Driver.Read(pd, "maxHealth"));
                if (hp <= 2 && hp > 0) Driver.Write(pd, "health", max);
            }
            if (calls % 30 == 1) { actors.Clear(); actors.AddRange(UnityEngine.Object.FindObjectsOfType<HealthManager>()); }
            Vector3 pos = hero.transform.position;
            if (route && gate == null)
            {
                Type type = Type.GetType("TransitionPoint, Assembly-CSharp");
                foreach (UnityEngine.Object o in UnityEngine.Object.FindObjectsOfType(type, true))
                {
                    Component c = o as Component;
                    if (c != null && c.gameObject.scene.name == scene && c.name == gates[routeIndex]) { gate = c; break; }
                }
                if (gate == null) { Finish("no gate " + gates[routeIndex] + " in " + scene); return; }
                goalX = gate.transform.position.x;
                expectScene = Driver.Number(Driver.Read(gate, "targetScene"));
                expectEntry = Driver.Number(Driver.Read(gate, "entryPoint"));
            }
            if (route && calls - sceneStart > sceneBudget)
            {
                // Stuck in a vertical or gated room: leave by the same gate, as the game would after the walk.
                string to = Driver.Number(Driver.Read(gate, "targetScene")), entry = Driver.Number(Driver.Read(gate, "entryPoint"));
                Note("budget spent in " + scene + ", direct load " + to + ":" + entry + " hero at " + pos.x.ToString("F1", CultureInfo.InvariantCulture) + "," + pos.y.ToString("F1", CultureInfo.InvariantCulture));
                Driver.Write(gm, "entryGateName", entry);
                ReplayDevice.SetButtons(0);
                Driver.Call(gm, "LoadScene", to);
                sceneStart = calls;
                return;
            }
            if (talk)
            {
                if (npc == null)
                {
                    if (talked.Count >= talkCount) { Finish("talked to " + talked.Count); return; }
                    float bestDx = float.MaxValue;
                    foreach (PlayMakerFSM f in UnityEngine.Object.FindObjectsOfType<PlayMakerFSM>())
                    {
                        if (f.FsmName != "Conversation Control" || !f.gameObject.activeInHierarchy || talked.Contains(f.GetInstanceID())) continue;
                        float d = Mathf.Abs(f.transform.position.x - pos.x) + Mathf.Abs(f.transform.position.y - pos.y);
                        if (d < bestDx) { bestDx = d; npc = f; }
                    }
                    if (npc == null) { Finish("no more NPCs after " + talked.Count); return; }
                    npcSince = calls;
                    npcArrive = 0;
                    Note("walking to NPC " + npc.gameObject.name + " at " + npc.transform.position.x.ToString("F1", CultureInfo.InvariantCulture));
                }
                float ndx = npc.transform.position.x - pos.x;
                uint tm = 0;
                if (Mathf.Abs(ndx) > 2.5f && calls - npcSince < 1500)
                {
                    tm |= ndx > 0 ? ReplayDevice.Right : ReplayDevice.Left;
                    if (!float.IsNaN(lastX) && Mathf.Abs(pos.x - lastX) < 0.01f) stillFrames++; else stillFrames = 0;
                    lastX = pos.x;
                    if (jumpHold > 0) { jumpHold--; tm |= ReplayDevice.Jump; }
                    else if (stillFrames > 10) { jumpHold = 16 + rng.Next(14); stillFrames = 0; tm |= ReplayDevice.Jump; }
                }
                else
                {
                    // In range: Up opens the conversation, Jump moves it on.
                    if (npcArrive == 0) npcArrive = calls;
                    int k = calls % 40;
                    if (k < 6) tm |= ReplayDevice.Up;
                    else if (k >= 20 && k < 24) tm |= ReplayDevice.Jump;
                    if (calls - npcArrive > 900) { talked.Add(npc.GetInstanceID()); Note("done with NPC " + npc.gameObject.name); npc = null; }
                }
                ReplayDevice.SetButtons(tm);
                return;
            }
            Rigidbody2D body = hero.GetComponent<Rigidbody2D>();
            HealthManager target = null;
            float bd = float.MaxValue;
            foreach (HealthManager m in actors)
            {
                if (m == null || !m.gameObject.activeInHierarchy || m.isDead || m.hp <= 0) continue;
                if (filter != null && !Matches(m.name)) continue;
                // Test setup: a matching enemy starts with this many hit points, so a whole fight fits a short run.
                if (bossHp > 0 && m.hp > bossHp && trimmed.Add(m.GetInstanceID())) { m.hp = bossHp; Note("enemy hp " + m.name + " -> " + bossHp); }
                Vector3 q = m.transform.position;
                float d = Mathf.Abs(q.x - pos.x) + 2f * Mathf.Abs(q.y - pos.y);
                if (d < bd) { bd = d; target = m; }
            }
            uint mask = 0;
            bool near = target != null && Mathf.Abs(target.transform.position.x - pos.x) < 3.2f
                && Mathf.Abs(target.transform.position.y - pos.y) < 4f;
            bool swing = near;
            float aimX, aimY = pos.y;
            if (fight)
            {
                if (target == null && !hasFallback) { ReplayDevice.SetButtons(0); return; }
                // fight:Name@X goes to X first (a boss that sleeps above the arena is a target from the start).
                if (hasFallback && Mathf.Abs(pos.x - goalX) < 2f) reached = true;
                if (hasFallback && !reached) { target = null; swing = false; }
                aimX = target != null ? target.transform.position.x : goalX;
                if (target != null) aimY = target.transform.position.y;
                float dx = aimX - pos.x;
                if (Mathf.Abs(dx) > 2.4f) mask |= dx > 0 ? ReplayDevice.Right : ReplayDevice.Left;
                else if (calls - lastTurn > 18) { mask |= dx > 0 ? ReplayDevice.Right : ReplayDevice.Left; lastTurn = calls; }
                if (target != null && target.transform.position.y - pos.y > 2.5f) mask |= ReplayDevice.Up;
            }
            else
            {
                aimX = goalX;
                if (route && gate != null) aimY = gate.transform.position.y;
                float dx = goalX - pos.x;
                if (Mathf.Abs(dx) > 1.5f) { mask |= dx > 0 ? ReplayDevice.Right : ReplayDevice.Left; goalFrames = 0; }
                else if (!route && ++goalFrames > 30) { Finish("goal reached"); return; }
                else if (aimY - pos.y > 3f && rng.Next(40) == 0) jumpHold = 20;
            }
            // Progress watchdog: a long time without getting nearer to the aim point means a wall or a ledge.
            float dist = Mathf.Abs(aimX - pos.x) + Mathf.Abs(aimY - pos.y);
            if (dist < best - 1.0f || swing) { best = Mathf.Min(best, dist); bestAt = calls; }
            if (wander == 0 && calls - bestAt > 150) { wander = 110; wanderDir = rng.Next(2) == 0 ? -1 : 1; bestAt = calls; best = dist; }
            if (wander > 0)
            {
                wander--;
                mask &= ~(ReplayDevice.Left | ReplayDevice.Right);
                mask |= wanderDir > 0 ? ReplayDevice.Right : ReplayDevice.Left;
                if (wander % 22 == 0) jumpHold = 16;
            }
            // Blocked or pinned: jump.
            if (!float.IsNaN(lastX) && Mathf.Abs(pos.x - lastX) < 0.01f && (mask & (ReplayDevice.Left | ReplayDevice.Right)) != 0) stillFrames++;
            else stillFrames = 0;
            lastX = pos.x;
            if (jumpHold > 0) { jumpHold--; mask |= ReplayDevice.Jump; }
            else if (stillFrames > 12 || (swing && rng.Next(120) == 0) || (!swing && rng.Next(150) == 0)) { jumpHold = 14 + rng.Next(14); stillFrames = 0; mask |= ReplayDevice.Jump; }
            // Swing in 3-frame pulses.
            if (swing)
            {
                attackHold = (attackHold + 1) % 8;
                if (attackHold < 3) mask |= ReplayDevice.Attack;
            }
            ReplayDevice.SetButtons(mask);
        }
    }
}
