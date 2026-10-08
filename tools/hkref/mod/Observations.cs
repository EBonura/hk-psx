// Read-only, sampled observations of the isolated Windows reference game.
// A sample can miss intermediate FSM states and short-lived/spawned objects.
// AudioSource configuration/isPlaying is NOT an interception of PlayOneShot.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Reflection;
using UnityEngine;
using UnityEngine.SceneManagement;

namespace HKReference
{
    public static class Observations
    {
        private const int FsmLimit = 2048, ActorLimit = 1024, AudioLimit = 512;
        private const int RescanInterval = 30;
        private static readonly FieldInfo Hp = typeof(HealthManager).GetField("hp", BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic);
        private static readonly FieldInfo Dead = typeof(HealthManager).GetField("isDead", BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic);
        private static readonly Dictionary<int, Entry> Entries = new Dictionary<int, Entry>();
        private static readonly List<int> Removed = new List<int>();
        private static StreamWriter writer;
        private static bool dirty;
        private static long captures;

        private sealed class Entry
        {
            public Component Component;
            public string Kind, Scene, Path, Type, Previous;
            public int Id;
        }

        public static void Initialize(string output)
        {
            Dispose();
            if (String.IsNullOrEmpty(output)) throw new ArgumentException("Missing reference output directory");
            Directory.CreateDirectory(output);
            writer = new StreamWriter(Path.Combine(output, "observations.csv"), false);
            writer.AutoFlush = true;
            writer.WriteLine("test_frame,unity_frame,time,fixed_time,event,kind,scene,hierarchy,instance_id,component_type,active,enabled,x,y,z,fsm_name,state,hp,dead,clip,clip_instance_id,pitch,volume,is_playing,loop,detail");
            UnityEngine.SceneManagement.SceneManager.sceneLoaded += Loaded;
            UnityEngine.SceneManagement.SceneManager.sceneUnloaded += Unloaded;
            captures = 0;
            dirty = true;
            Notice(-1, "coverage", "Polling once per Capture; FSM/actor/audio state changes only. Discovery on scene load and every 30 captures. Intermediate FSM states, one-shots, and objects destroyed between scans can be missed; audio rows describe configuration, not playback calls.");
        }

        public static void Capture(int testFrame)
        {
            if (writer == null) return;
            Removed.Clear();
            foreach (KeyValuePair<int, Entry> pair in Entries)
            {
                if (pair.Value.Component != null) continue;
                Write(testFrame, "destroyed", pair.Value, null);
                Removed.Add(pair.Key);
            }
            foreach (int id in Removed) Entries.Remove(id);
            if (dirty || captures % RescanInterval == 0)
            {
                dirty = false;
                Discover(UnityEngine.Object.FindObjectsByType<PlayMakerFSM>(FindObjectsInactive.Include, FindObjectsSortMode.None), "fsm", FsmLimit, testFrame);
                Discover(UnityEngine.Object.FindObjectsByType<HealthManager>(FindObjectsInactive.Include, FindObjectsSortMode.None), "actor", ActorLimit, testFrame);
                Discover(UnityEngine.Object.FindObjectsByType<AudioSource>(FindObjectsInactive.Include, FindObjectsSortMode.None), "audio", AudioLimit, testFrame);
            }
            foreach (Entry entry in Entries.Values)
            {
                if (entry.Component == null) continue;
                string[] values = Values(entry);
                // Moving FSM/audio owners do not produce rows unless their
                // observed state/configuration changes; actors retain poses.
                string signature = entry.Kind == "actor" ? Join(values) :
                    Csv(values[0]) + "," + Csv(values[1]) + "," + Join(values, 5);
                if (signature == entry.Previous) continue;
                Write(testFrame, entry.Previous == null ? "discovered" : "changed", entry, values);
                entry.Previous = signature;
            }
            captures++;
        }

        private static void Discover<T>(T[] objects, string kind, int limit, int frame) where T : Component
        {
            // Instance ordering makes a truncated scan deterministic within a
            // process. Overflow is explicit, never silently complete coverage.
            Array.Sort(objects, (a, b) => a.GetInstanceID().CompareTo(b.GetInstanceID()));
            int count = 0, retained = 0;
            foreach (Entry existing in Entries.Values) if (existing.Kind == kind) retained++;
            foreach (T component in objects)
            {
                if (component == null || !component.gameObject.scene.IsValid() || !component.gameObject.scene.isLoaded) continue;
                count++;
                int id = component.GetInstanceID();
                if (Entries.ContainsKey(id)) continue;
                if (retained >= limit) continue;
                Entries.Add(id, new Entry {
                    Component = component, Id = id, Kind = kind,
                    Scene = component.gameObject.scene.name,
                    Path = Hierarchy(component.transform), Type = component.GetType().FullName
                });
                retained++;
            }
            if (count > limit) Notice(frame, "coverage_limit", kind + ": found=" + N(count) + " tracked_limit=" + N(limit));
        }

        private static string[] Values(Entry entry)
        {
            Component component = entry.Component;
            Vector3 p = component.transform.position;
            Behaviour behaviour = component as Behaviour;
            string[] values = new string[15];
            values[0] = N(component.gameObject.activeInHierarchy);
            values[1] = behaviour == null ? "" : N(behaviour.enabled);
            values[2] = N(p.x); values[3] = N(p.y); values[4] = N(p.z);
            PlayMakerFSM fsm = component as PlayMakerFSM;
            if (fsm != null) { values[5] = fsm.FsmName; values[6] = fsm.ActiveStateName; }
            HealthManager actor = component as HealthManager;
            if (actor != null)
            {
                values[7] = Hp == null ? "" : N(Hp.GetValue(actor));
                values[8] = Dead == null ? "" : N(Dead.GetValue(actor));
            }
            AudioSource audio = component as AudioSource;
            if (audio != null)
            {
                AudioClip clip = audio.clip;
                values[9] = clip == null ? "" : clip.name;
                values[10] = clip == null ? "" : N(clip.GetInstanceID());
                values[11] = N(audio.pitch); values[12] = N(audio.volume);
                values[13] = N(audio.isPlaying); values[14] = N(audio.loop);
            }
            return values;
        }

        private static void Write(int frame, string action, Entry entry, string[] values)
        {
            string[] row = new string[26];
            row[0] = N(frame); row[1] = N(Time.frameCount); row[2] = N(Time.time); row[3] = N(Time.fixedTime);
            row[4] = action; row[5] = entry.Kind; row[6] = entry.Scene; row[7] = entry.Path;
            row[8] = N(entry.Id); row[9] = entry.Type;
            if (values != null) Array.Copy(values, 0, row, 10, values.Length);
            writer.WriteLine(Join(row));
        }

        private static void Notice(int frame, string action, string detail)
        {
            string[] row = new string[26];
            row[0] = N(frame); row[1] = N(Time.frameCount); row[2] = N(Time.time); row[3] = N(Time.fixedTime);
            row[4] = action; row[25] = detail;
            writer.WriteLine(Join(row));
        }

        private static string Hierarchy(Transform transform)
        {
            List<string> parts = new List<string>();
            for (Transform t = transform; t != null; t = t.parent)
                parts.Add(t.name.Replace("\\", "\\\\").Replace("/", "\\/") + "[" + N(t.GetSiblingIndex()) + "]");
            parts.Reverse();
            return String.Join("/", parts.ToArray());
        }

        private static string N(object value)
        {
            if (value == null) return "";
            if (value is float) return ((float)value).ToString("R", CultureInfo.InvariantCulture);
            return Convert.ToString(value, CultureInfo.InvariantCulture);
        }
        private static string Csv(string value)
        {
            value = value ?? "";
            return value.IndexOfAny(new char[] { ',', '"', '\r', '\n' }) < 0 ? value : "\"" + value.Replace("\"", "\"\"") + "\"";
        }
        private static string Join(string[] values, int start = 0)
        {
            string[] escaped = new string[values.Length - start];
            for (int i = start; i < values.Length; i++) escaped[i - start] = Csv(values[i]);
            return String.Join(",", escaped);
        }
        private static void Loaded(Scene scene, LoadSceneMode mode) { dirty = true; }
        private static void Unloaded(Scene scene) { dirty = true; }
        public static void Dispose()
        {
            UnityEngine.SceneManagement.SceneManager.sceneLoaded -= Loaded;
            UnityEngine.SceneManagement.SceneManager.sceneUnloaded -= Unloaded;
            if (writer != null) { writer.Dispose(); writer = null; }
            Entries.Clear(); Removed.Clear();
        }
    }
}
