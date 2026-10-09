// Scene survey: visit scenes of the original one by one and record what each holds.
// Per scene: every HealthManager (active or not) with its position, hit points and
// PlayMaker state; every clip an FSM action can play, with the FSM, state and
// incoming events; every AudioSource; the exits. Then a short tour: the hero is
// moved next to each distinct enemy in turn (invulnerable) so its behaviour and
// sounds run, and the timeline says which window belongs to which enemy.
// Test setup only: the game's own logic still produces everything recorded.
using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Text;
using HutongGames.PlayMaker;
using UnityEngine;

namespace HKReference
{
    public sealed class SceneSurvey
    {
        private enum Phase { Idle, Loading, Settle, Tour, Poke }

        private sealed class Target { public string Name; public float X, Y; public HealthManager Hm; }

        private readonly Queue<string> pending = new Queue<string>();
        private readonly string stateDir;
        private readonly int settleFrames, tourFrames, maxTargets, loadTimeout, pokeFrames, pokeFirst, pokeGap, pokeMaxHits;
        private readonly float pokeDirection;
        private readonly StreamWriter timeline, actorsOut, fsmOut, sourcesOut, scenesOut, pokesOut;
        private int pokeIndex, pokeStart, pokeHits, pokeLength;
        private Phase phase = Phase.Idle;
        private string current;
        private int phaseStart, stable, loadFrames, targetStart, tourIndex;
        private string loadStatus = "ok";
        private List<Target> targets = new List<Target>();
        private bool invincible;
        private string sceneGates = "";
        private readonly List<Vector2> gatePositions = new List<Vector2>();
        private string windowName = "";
        private float windowX, windowY;

        public static SceneSurvey FromEnvironment(string output)
        {
            string spec = Environment.GetEnvironmentVariable("HK_REFERENCE_SURVEY");
            if (String.IsNullOrEmpty(spec)) return null;
            return new SceneSurvey(output, spec);
        }

        private SceneSurvey(string output, string spec)
        {
            stateDir = Environment.GetEnvironmentVariable("HK_REFERENCE_SURVEY_STATE");
            if (String.IsNullOrEmpty(stateDir)) stateDir = output;
            Directory.CreateDirectory(stateDir);
            settleFrames = Setting("HK_REFERENCE_SURVEY_FRAMES", 60);
            tourFrames = Setting("HK_REFERENCE_SURVEY_TOUR_FRAMES", 75);
            maxTargets = Setting("HK_REFERENCE_SURVEY_TARGETS", 12);
            loadTimeout = Setting("HK_REFERENCE_SURVEY_LOAD_TIMEOUT", 900);
            // Poke mode: instead of touring, strike each distinct enemy with the Knight's nail (5 damage,
            // from the left unless HK_REFERENCE_POKE_DIRECTION says otherwise) every POKE_GAP frames and
            // record what it does. The strikes go through the game's own HealthManager.Hit.
            pokeFrames = Setting("HK_REFERENCE_SURVEY_POKE", 0);
            pokeFirst = Setting("HK_REFERENCE_POKE_FIRST", 30);
            pokeGap = Setting("HK_REFERENCE_POKE_GAP", 30);
            pokeMaxHits = Setting("HK_REFERENCE_POKE_MAX_HITS", 8);
            float dir; pokeDirection = Single.TryParse(Environment.GetEnvironmentVariable("HK_REFERENCE_POKE_DIRECTION"), NumberStyles.Float, CultureInfo.InvariantCulture, out dir) ? dir : 0f;
            ActorTrace.Stride = Setting("HK_REFERENCE_SURVEY_ACTOR_STRIDE", 5);
            HashSet<string> done = new HashSet<string>();
            string donePath = Path.Combine(stateDir, "survey-done.txt");
            string currentPath = Path.Combine(stateDir, "survey-current.txt");
            // A scene that was in progress when the process died is the one that crashed it.
            if (File.Exists(currentPath))
            {
                string crashed = File.ReadAllText(currentPath).Trim();
                if (crashed.Length > 0) File.AppendAllText(donePath, crashed + "\tcrash\n");
                File.Delete(currentPath);
            }
            if (File.Exists(donePath))
                foreach (string line in File.ReadAllLines(donePath)) done.Add(line.Split('\t')[0]);
            List<string> all = new List<string>();
            for (int i = 0; i < UnityEngine.SceneManagement.SceneManager.sceneCountInBuildSettings; i++)
                all.Add(Path.GetFileNameWithoutExtension(UnityEngine.SceneManagement.SceneUtility.GetScenePathByBuildIndex(i)));
            string[] parts = spec.Split(',');
            foreach (string raw in parts)
            {
                string p = raw.Trim();
                if (p.Length == 0) continue;
                bool remove = p.StartsWith("-");
                if (remove) p = p.Substring(1);
                List<string> matched;
                if (p == "*") matched = all;
                else if (p.EndsWith("*")) matched = all.Where(s => s.StartsWith(p.TrimEnd('*'), StringComparison.Ordinal)).ToList();
                else matched = all.Where(s => s == p).ToList();
                if (remove) { List<string> keep = pending.Where(s => !matched.Contains(s)).ToList(); pending.Clear(); foreach (string s in keep) pending.Enqueue(s); }
                else foreach (string s in matched) if (!pending.Contains(s)) pending.Enqueue(s);
            }
            if (done.Count > 0)
            {
                List<string> left = pending.Where(s => !done.Contains(s)).ToList();
                pending.Clear();
                foreach (string s in left) pending.Enqueue(s);
            }
            File.WriteAllLines(Path.Combine(output, "survey-buildscenes.csv"), all.ToArray());
            timeline = Open(output, "survey-timeline.csv", "scene,phase,target,x,y,start_frame,end_frame");
            actorsOut = Open(output, "survey-actors.csv", "scene,id,name,path,x,y,z,hp,active_in_hierarchy,active_self,enemy_type,fsms,clips,fsm_states");
            fsmOut = Open(output, "survey-fsm-audio.csv", "scene,kind,object,fsm,state,action,clips_or_objects,params,incoming_events");
            sourcesOut = Open(output, "survey-audiosources.csv", "scene,object,clip,loop,play_on_awake,volume,spatial_blend,min_distance,max_distance,mixer_group,active_in_hierarchy,enabled");
            scenesOut = Open(output, "survey-scenes.csv", "scene,status,load_frames,hero_x,hero_y,gates");
            pokesOut = Open(output, "survey-pokes.csv", "scene,target,id,frame,rel,direction,hp_before,hp_after,dead,x,y");
        }

        private static StreamWriter Open(string dir, string name, string header)
        {
            StreamWriter w = new StreamWriter(Path.Combine(dir, name), false);
            w.AutoFlush = true;
            w.WriteLine(header);
            return w;
        }

        private static int Setting(string name, int fallback)
        {
            int v;
            return Int32.TryParse(Environment.GetEnvironmentVariable(name), out v) && v > 0 ? v : fallback;
        }

        public bool Busy { get { return phase != Phase.Idle; } }

        // One call per observed test frame. True when every scene is done.
        public bool Step(int frame, object gm, Component hero)
        {
            string active = UnityEngine.SceneManagement.SceneManager.GetActiveScene().name;
            switch (phase)
            {
                case Phase.Idle:
                    if (pending.Count == 0) return true;
                    current = pending.Dequeue();
                    File.WriteAllText(Path.Combine(stateDir, "survey-current.txt"), current);
                    Driver.Write(gm, "entryGateName", "zzz");
                    ReplayDevice.SetButtons(0);
                    Freeze(hero, true);
                    Driver.Call(gm, "LoadScene", current);
                    phase = Phase.Loading; phaseStart = frame; stable = 0; loadStatus = "ok";
                    return false;
                case Phase.Loading:
                {
                    // Loaded, not necessarily playing: with no real entry gate the game waits in
                    // ENTERING_LEVEL for a hero animation that never comes (see SetPlaying).
                    bool ok = active == current && hero != null && Driver.Number(Driver.Read(gm, "isLoading")) == "False";
                    stable = ok ? stable + 1 : 0;
                    if (stable >= 20 || frame - phaseStart > loadTimeout)
                    {
                        if (stable < 20) loadStatus = active == current ? "no_hero" : "load_timeout";
                        loadFrames = frame - phaseStart;
                        phase = Phase.Settle; phaseStart = frame;
                        BeginWindow("idle", "", 0, 0, frame);
                        Freeze(hero, true);
                        if (!invincible && ok) MakeInvincible(gm);
                    }
                    return false;
                }
                case Phase.Settle:
                    if (frame - phaseStart < settleFrames) return false;
                    EndWindow(frame);
                    try { Census(current, hero, gm); }
                    catch (Exception e) { loadStatus += "+census_error:" + e.GetType().Name; Debug.Log("HKReference census failed in " + current + ": " + e); }
                    if (loadStatus != "ok" || targets.Count == 0) { Finish(); return false; }
                    if (pokeFrames > 0) { pokeIndex = 0; phase = Phase.Poke; SetPlaying(gm); StartPoke(frame, hero); return false; }
                    tourIndex = 0; phase = Phase.Tour; SetPlaying(gm); StartTarget(frame, hero);
                    return false;
                case Phase.Poke:
                {
                    if (active != current) { EndWindow(frame); loadStatus += "+left_scene"; Finish(); return false; }
                    Target pt = targets[pokeIndex];
                    int rel = frame - pokeStart;
                    if (pt.Hm != null && pokeHits < pokeMaxHits && rel >= pokeFirst && (rel - pokeFirst) % pokeGap == 0)
                        Poke(pt, frame, rel, hero);
                    if (rel < pokeLength) return false;
                    EndWindow(frame);
                    pokeIndex++;
                    if (pokeIndex >= targets.Count) { Finish(); return false; }
                    StartPoke(frame, hero);
                    return false;
                }
                case Phase.Tour:
                    if (active != current) { EndWindow(frame); loadStatus += "+left_scene"; Finish(); return false; }
                    if (frame - targetStart < tourFrames) return false;
                    EndWindow(frame);
                    tourIndex++;
                    if (tourIndex >= targets.Count) { Finish(); return false; }
                    StartTarget(frame, hero);
                    return false;
            }
            return false;
        }

        // The scene was entered through a gate that does not exist, so the game sits in
        // ENTERING_LEVEL. Put it in PLAYING when the tour starts (never before: a hero
        // standing in a gate's trigger would leave the scene).
        // A frozen hero has no physics and so no trigger events: it cannot walk into a gate
        // (or fall into a pit) while a scene settles. Thawed per tour target.
        private static void Freeze(Component hero, bool on)
        {
            if (hero == null) return;
            Rigidbody2D body = hero.GetComponent<Rigidbody2D>();
            if (body == null) return;
            if (on) body.velocity = Vector2.zero;
            body.simulated = !on;
        }

        private void SetPlaying(object gm)
        {
            try
            {
                if (Driver.Number(Driver.Read(gm, "gameState")) == "PLAYING") return;
                Type state = Type.GetType("GlobalEnums.GameState, Assembly-CSharp");
                if (state != null) Driver.Call(gm, "SetState", Enum.Parse(state, "PLAYING"));
            }
            catch (Exception e) { loadStatus += "+set_playing_error:" + e.GetType().Name; }
        }

        private void MakeInvincible(object gm)
        {
            try { Driver.Write(Driver.Read(gm, "playerData"), "isInvincible", true); invincible = true; }
            catch (Exception) { }
        }

        private void Finish()
        {
            Component hero = Driver.Singleton("HeroController") as Component;
            scenesOut.WriteLine(String.Join(",", new string[] { current, loadStatus, loadFrames.ToString(CultureInfo.InvariantCulture),
                hero != null ? F(hero.transform.position.x) : "", hero != null ? F(hero.transform.position.y) : "", Quote(sceneGates) }));
            File.AppendAllText(Path.Combine(stateDir, "survey-done.txt"), current + "\t" + loadStatus + "\n");
            File.Delete(Path.Combine(stateDir, "survey-current.txt"));
            Freeze(hero, true);
            phase = Phase.Idle;
        }

        private void StartTarget(int frame, Component hero)
        {
            Target t = targets[tourIndex];
            targetStart = frame;
            BeginWindow("tour", t.Name, t.X, t.Y, frame);
            if (hero != null)
            {
                Vector3 p = hero.transform.position;
                hero.transform.position = new Vector3(t.X, t.Y + 0.5f, p.z);
                Rigidbody2D body = hero.GetComponent<Rigidbody2D>();
                if (body != null) body.velocity = Vector2.zero;
                Freeze(hero, false);
            }
        }

        private void StartPoke(int frame, Component hero)
        {
            Target t = targets[pokeIndex];
            pokeStart = frame; pokeHits = 0;
            // Enough hits to kill it plus one, but no more than the cap; then time for the corpse.
            int hp = t.Hm != null ? Math.Max(1, t.Hm.hp) : 1;
            int hits = Math.Min(pokeMaxHits, (hp + 4) / 5 + 1);
            pokeLength = pokeFirst + hits * pokeGap + pokeFrames;
            Freeze(hero, true);
            BeginWindow("poke", t.Name, t.X, t.Y, frame);
        }

        private void Poke(Target t, int frame, int rel, Component hero)
        {
            HealthManager hm = t.Hm;
            if (hm == null || hm.gameObject == null || !hm.gameObject.activeInHierarchy) return;
            Vector3 p = hm.transform.position;
            int before = hm.hp;
            HitInstance hit = new HitInstance();
            hit.Source = hero != null ? hero.gameObject : null;
            hit.AttackType = AttackTypes.Nail;
            hit.DamageDealt = 5;
            hit.Direction = pokeDirection;
            hit.MagnitudeMultiplier = 1f;
            hit.Multiplier = 1f;
            hm.Hit(hit);
            pokeHits++;
            pokesOut.WriteLine(String.Join(",", new string[] { current, Quote(t.Name), hm.GetInstanceID().ToString(CultureInfo.InvariantCulture), frame.ToString(CultureInfo.InvariantCulture),
                rel.ToString(CultureInfo.InvariantCulture), F(pokeDirection), before.ToString(CultureInfo.InvariantCulture), hm.hp.ToString(CultureInfo.InvariantCulture), hm.isDead ? "1" : "0", F(p.x), F(p.y) }));
        }

        private void BeginWindow(string name, string target, float x, float y, int frame) { windowName = name + "|" + target; windowX = x; windowY = y; windowStart = frame; }
        private int windowStart;
        private void EndWindow(int frame)
        {
            string[] w = windowName.Split('|');
            timeline.WriteLine(String.Join(",", new string[] { current, w[0], Quote(w.Length > 1 ? w[1] : ""), F(windowX), F(windowY),
                windowStart.ToString(CultureInfo.InvariantCulture), frame.ToString(CultureInfo.InvariantCulture) }));
        }

        private string Gates(string scene)
        {
            gatePositions.Clear();
            Type type = Type.GetType("TransitionPoint, Assembly-CSharp");
            if (type == null) return "";
            StringBuilder sb = new StringBuilder();
            foreach (UnityEngine.Object o in UnityEngine.Object.FindObjectsOfType(type, true))
            {
                Component c = o as Component;
                if (c == null || c.gameObject.scene.name != scene) continue;
                gatePositions.Add(new Vector2(c.transform.position.x, c.transform.position.y));
                sb.Append(c.name).Append('@').Append(F(c.transform.position.x)).Append(' ').Append(F(c.transform.position.y))
                  .Append("->").Append(Driver.Number(Driver.Read(c, "targetScene"))).Append(':').Append(Driver.Number(Driver.Read(c, "entryPoint"))).Append(';');
            }
            return sb.ToString();
        }

        private static string BaseName(string n)
        {
            n = n.Replace("(Clone)", "").Trim();
            int p = n.LastIndexOf(" (", StringComparison.Ordinal);
            if (p > 0 && n.EndsWith(")")) n = n.Substring(0, p);
            n = n.TrimEnd();
            int e = n.Length;
            while (e > 1 && Char.IsDigit(n[e - 1])) e--;
            if (e > 0 && e < n.Length && n[e - 1] == ' ') n = n.Substring(0, e - 1);
            return n.Trim();
        }

        private void Census(string scene, Component hero, object gm)
        {
            targets = new List<Target>();
            sceneGates = Gates(scene);
            HashSet<string> seen = new HashSet<string>();
            List<Target> picked = new List<Target>();
            foreach (HealthManager m in UnityEngine.Object.FindObjectsOfType<HealthManager>(true))
            {
                if (m == null || m.gameObject.scene.name != scene) continue;
                Vector3 p = m.transform.position;
                StringBuilder fsms = new StringBuilder(), allStates = new StringBuilder();
                foreach (PlayMakerFSM f in m.GetComponents<PlayMakerFSM>())
                {
                    fsms.Append(f.FsmName).Append('=').Append(f.ActiveStateName).Append(';');
                    allStates.Append(f.FsmName).Append('=');
                    if (f.Fsm != null && f.Fsm.States != null) allStates.Append(String.Join("|", f.Fsm.States.Select(st => st.Name).ToArray()));
                    allStates.Append(';');
                }
                List<string> clips = new List<string>();
                foreach (MonoBehaviour c in m.GetComponents<MonoBehaviour>())
                    if (c != null) FieldClips(c, c.GetType().Name, clips);
                actorsOut.WriteLine(String.Join(",", new string[] { scene, m.GetInstanceID().ToString(CultureInfo.InvariantCulture), Quote(m.name), Quote(PathOf(m.transform)),
                    F(p.x), F(p.y), F(p.z), m.hp.ToString(CultureInfo.InvariantCulture), m.gameObject.activeInHierarchy ? "1" : "0", m.gameObject.activeSelf ? "1" : "0",
                    Driver.Number(Driver.Read(m, "enemyType")), Quote(fsms.ToString()), Quote(String.Join("|", clips.ToArray())), Quote(allStates.ToString()) }));
                if (m.hp > 0 && !m.isDead && !NearGate(p) && seen.Add(BaseName(m.name))) picked.Add(new Target { Name = BaseName(m.name), X = p.x, Y = p.y, Hm = m });
            }
            picked.Sort((a, b) => a.X.CompareTo(b.X));
            targets = picked.Count <= maxTargets ? picked : Spread(picked, maxTargets);
            FsmInventory(scene);
            foreach (AudioSource s in UnityEngine.Object.FindObjectsOfType<AudioSource>(true))
            {
                if (s == null || s.gameObject.scene.name != scene) continue;
                sourcesOut.WriteLine(String.Join(",", new string[] { scene, Quote(PathOf(s.transform)), Quote(s.clip != null ? s.clip.name : ""), s.loop ? "1" : "0", s.playOnAwake ? "1" : "0",
                    F(s.volume), F(s.spatialBlend), F(s.minDistance), F(s.maxDistance), Quote(s.outputAudioMixerGroup != null ? s.outputAudioMixerGroup.name : ""),
                    s.gameObject.activeInHierarchy ? "1" : "0", s.enabled ? "1" : "0" }));
            }
        }

        // A hero teleported into a gate's trigger leaves the scene; keep clear of them.
        private bool NearGate(Vector3 p)
        {
            foreach (Vector2 g in gatePositions) if (Mathf.Abs(g.x - p.x) < 5f && Mathf.Abs(g.y - p.y) < 8f) return true;
            return false;
        }

        private static List<Target> Spread(List<Target> all, int n)
        {
            List<Target> r = new List<Target>();
            for (int i = 0; i < n; i++) r.Add(all[i * (all.Count - 1) / Math.Max(1, n - 1)]);
            return r.Distinct().ToList();
        }

        private void FsmInventory(string scene)
        {
            foreach (PlayMakerFSM f in UnityEngine.Object.FindObjectsOfType<PlayMakerFSM>(true))
            {
                if (f == null || f.gameObject.scene.name != scene) continue;
                Fsm fsm = f.Fsm;
                if (fsm == null || fsm.States == null) continue;
                string path = PathOf(f.transform);
                foreach (FsmState st in fsm.States)
                {
                    if (st.Actions == null) continue;
                    foreach (FsmStateAction a in st.Actions)
                    {
                        if (a == null) continue;
                        List<string> clips = new List<string>(), objs = new List<string>(), pars = new List<string>();
                        string type = a.GetType().Name;
                        bool spawn = type.IndexOf("Spawn", StringComparison.OrdinalIgnoreCase) >= 0 || type.IndexOf("CreateObject", StringComparison.OrdinalIgnoreCase) >= 0;
                        foreach (FieldInfo field in a.GetType().GetFields(BindingFlags.Public | BindingFlags.Instance))
                        {
                            object v;
                            try { v = field.GetValue(a); } catch (Exception) { continue; }
                            Collect(v, field.Name, clips, 0);
                            if (spawn) { FsmGameObject go = v as FsmGameObject; if (go != null && go.Value != null) objs.Add(field.Name + "=" + go.Value.name); }
                            string lower = field.Name.ToLowerInvariant();
                            if (lower.Contains("pitch") || lower.Contains("volume") || lower.Contains("delay") || lower.Contains("loop"))
                            {
                                FsmFloat ff = v as FsmFloat; FsmBool fb = v as FsmBool;
                                if (ff != null) pars.Add(field.Name + "=" + F(ff.Value));
                                else if (fb != null) pars.Add(field.Name + "=" + fb.Value);
                                else if (v is float) pars.Add(field.Name + "=" + F((float)v));
                            }
                        }
                        if (clips.Count == 0 && objs.Count == 0) continue;
                        string incoming = Incoming(fsm, st);
                        if (clips.Count > 0)
                            fsmOut.WriteLine(String.Join(",", new string[] { scene, "audio", Quote(path), Quote(f.FsmName), Quote(st.Name), type, Quote(String.Join("|", clips.ToArray())), Quote(String.Join(";", pars.ToArray())), Quote(incoming) }));
                        if (objs.Count > 0)
                            fsmOut.WriteLine(String.Join(",", new string[] { scene, "spawn", Quote(path), Quote(f.FsmName), Quote(st.Name), type, Quote(String.Join("|", objs.ToArray())), "", Quote(incoming) }));
                    }
                }
            }
        }

        private static string Incoming(Fsm fsm, FsmState target)
        {
            List<string> r = new List<string>();
            foreach (FsmState s in fsm.States)
                if (s.Transitions != null)
                    foreach (FsmTransition t in s.Transitions)
                        if (t.ToState == target.Name && r.Count < 6) r.Add(s.Name + ":" + (t.FsmEvent != null ? t.FsmEvent.Name : ""));
            if (fsm.GlobalTransitions != null)
                foreach (FsmTransition t in fsm.GlobalTransitions)
                    if (t.ToState == target.Name && r.Count < 8) r.Add("*:" + (t.FsmEvent != null ? t.FsmEvent.Name : ""));
            return String.Join(";", r.ToArray());
        }

        // Clip names reachable from an action field: AudioClip, FsmObject holding one, arrays, lists, FsmArray, AudioEvent-like structs.
        private static void Collect(object v, string label, List<string> clips, int depth)
        {
            if (v == null || depth > 2) return;
            AudioClip clip = v as AudioClip;
            if (clip != null) { clips.Add(label + "=" + clip.name); return; }
            FsmObject fo = v as FsmObject;
            if (fo != null) { clip = fo.Value as AudioClip; if (clip != null) clips.Add(label + "=" + clip.name); return; }
            FsmArray fa = v as FsmArray;
            if (fa != null) { if (fa.Values != null) foreach (object o in fa.Values) Collect(o, label, clips, depth + 1); return; }
            if (v is string || v.GetType().IsPrimitive || v is UnityEngine.Object) return;
            IEnumerable list = v as IEnumerable;
            if (list != null) { int n = 0; foreach (object o in list) { if (n++ > 64) break; Collect(o, label, clips, depth + 1); } return; }
            Type type = v.GetType();
            if (type.Name.IndexOf("Audio", StringComparison.OrdinalIgnoreCase) >= 0 && (type.IsValueType || type.IsClass))
                foreach (FieldInfo f in type.GetFields(BindingFlags.Public | BindingFlags.Instance))
                {
                    object x; try { x = f.GetValue(v); } catch (Exception) { continue; }
                    Collect(x, label, clips, depth + 1);
                }
        }

        // AudioClip-valued fields of a component, one level deep.
        private static void FieldClips(object component, string label, List<string> clips)
        {
            foreach (FieldInfo f in component.GetType().GetFields(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance))
            {
                if (f.FieldType.IsPrimitive || f.FieldType == typeof(string)) continue;
                object x; try { x = f.GetValue(component); } catch (Exception) { continue; }
                Collect(x, label + "." + f.Name, clips, 1);
            }
        }

        private static string PathOf(Transform t)
        {
            string path = t.name;
            for (Transform p = t.parent; p != null; p = p.parent) path = p.name + "/" + path;
            return path;
        }

        private static string F(float v) { return v.ToString("F4", CultureInfo.InvariantCulture); }
        private static string Quote(string text) { return "\"" + (text ?? "").Replace("\"", "'") + "\""; }

        public void Dispose()
        {
            // A clean stop in the middle of a scene: it is not a crash, the next process redoes it.
            if (phase != Phase.Idle) { try { File.Delete(Path.Combine(stateDir, "survey-current.txt")); } catch (Exception) { } }
            if (timeline != null) timeline.Dispose();
            if (actorsOut != null) actorsOut.Dispose();
            if (fsmOut != null) fsmOut.Dispose();
            if (sourcesOut != null) sourcesOut.Dispose();
            if (scenesOut != null) scenesOut.Dispose();
            if (pokesOut != null) pokesOut.Dispose();
        }
    }
}
