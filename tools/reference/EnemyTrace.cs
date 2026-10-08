// Read-only, sampled original-enemy evidence. No gameplay callbacks are invoked.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Reflection;
using UnityEngine;
using UnityEngine.SceneManagement;

namespace HKReference
{
    public static class EnemyTrace
    {
        private const int LimitPerType = 128, RescanInterval = 30;
        private static readonly Dictionary<int, Component> Actors = new Dictionary<int, Component>();
        private static readonly Dictionary<Type, Dictionary<string, FieldInfo>> Fields = new Dictionary<Type, Dictionary<string, FieldInfo>>();
        private static readonly List<int> Removed = new List<int>();
        private static StreamWriter writer, notices;
        private static bool dirty;
        private static int captures;
        private static readonly string[] Header = ("test_frame,unity_frame,time,fixed_time,physics_steps,scene,hierarchy,instance_id,controller,active,enabled,hp,dead,x,y,z,scale_x,rotation_z,body_x,body_y,body_rotation,vx,vy,clip,animation_previous_frame,animation_clip_time,animation_state,walker_state,walker_facing,walker_turning_facing,walker_stop_reason,walker_walk_remaining,walker_pause_remaining,walker_turn_cooldown,los_can_see_hero,alert_hero_in_range,climber_direction,climber_clockwise,climber_turn_handle_present,climber_previous_x,climber_previous_y,climber_previous_turn_x,climber_previous_turn_y,climber_phase,zombie_swipe_state,missing").Split(',');

        public static void Initialize(string output)
        {
            Dispose(); captures = 0; dirty = true;
            writer = new StreamWriter(Path.Combine(output, "enemy-trace.csv"), false);
            notices = new StreamWriter(Path.Combine(output, "enemy-trace-notices.log"), false);
            writer.WriteLine(String.Join(",", Header)); writer.Flush();
            Note("Sampled once per actual Driver.Capture (LateUpdate); cannot establish intermediate callback order, transient states or all spawns/deaths. Discovery every 30 captures and scene changes; at most 128 loaded actors per type. Unity discovery allocates its complete per-type result before this tracking cap. No gameplay methods or physics queries invoked. animation_previous_frame is the native animator backing field, not a recomputed frame. Climber phase is not exposed; turnRoutine handle presence alone does not establish phase.");
            UnityEngine.SceneManagement.SceneManager.sceneLoaded += Loaded; UnityEngine.SceneManagement.SceneManager.sceneUnloaded += Unloaded;
        }

        public static void Capture(int frame, int physicsSteps)
        {
            if (writer == null) return;
            try
            {
                Removed.Clear();
                foreach (var pair in Actors) if (pair.Value == null) Removed.Add(pair.Key);
                foreach (int id in Removed) { Actors.Remove(id); Note("destroyed sampled instance=" + id + " test_frame=" + frame); }
                if (dirty || captures % RescanInterval == 0)
                {
                    Actors.Clear(); Discover("Walker", frame); Discover("Climber", frame); dirty = false;
                }
                captures++;
                foreach (var pair in Actors) Sample(pair.Value, frame, physicsSteps);
                writer.Flush();
            }
            catch (Exception error)
            {
                Note("ERROR test_frame=" + frame + " " + error.GetType().Name + ": " + error.Message);
                Debug.LogError("HKReference EnemyTrace logging failed: " + error.Message);
            }
        }

        private static void Discover(string name, int frame)
        {
            Type type = typeof(HealthManager).Assembly.GetType(name);
            if (type == null) { Note("missing controller type " + name); return; }
            UnityEngine.Object[] found = UnityEngine.Object.FindObjectsByType(type, FindObjectsInactive.Include, FindObjectsSortMode.InstanceID);
            int loaded = 0;
            foreach (UnityEngine.Object obj in found)
            {
                Component actor = obj as Component;
                if (actor == null || !actor.gameObject.scene.IsValid() || !actor.gameObject.scene.isLoaded) continue;
                if (loaded++ < LimitPerType) Actors[actor.GetInstanceID()] = actor;
            }
            Note("scan test_frame=" + frame + " type=" + name + " loaded=" + loaded + " tracked=" + Math.Min(loaded, LimitPerType) + " omitted=" + Math.Max(0, loaded - LimitPerType));
        }

        // Exact private field reads only: no arbitrary properties, methods, or setters.
        private static object Read(object target, string name, List<string> missing, string label)
        {
            if (target == null || (target is UnityEngine.Object && (UnityEngine.Object)target == null))
            { missing.Add(label + ":null_component"); return null; }
            Type type = target.GetType(); Dictionary<string, FieldInfo> cache;
            if (!Fields.TryGetValue(type, out cache)) { cache = new Dictionary<string, FieldInfo>(); Fields.Add(type, cache); }
            FieldInfo field;
            if (!cache.TryGetValue(name, out field))
            {
                for (Type cursor = type; cursor != null && field == null; cursor = cursor.BaseType)
                    field = cursor.GetField(name, BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.DeclaredOnly);
                cache[name] = field;
            }
            if (field == null) { missing.Add(label + ":missing_field:" + name); return null; }
            try { return field.GetValue(target); }
            catch (Exception error) { missing.Add(label + ":" + error.GetType().Name); return null; }
        }

        private static void Sample(Component actor, int frame, int steps)
        {
            var missing = new List<string>(); var row = new string[Header.Length];
            Action<string, object> set = (key, value) => row[Array.IndexOf(Header, key)] = N(value);
            Func<object, string, object> read = (target, field) => Read(target, field, missing, (target == null ? "null" : target.GetType().Name) + "." + field);
            set("test_frame", frame); set("unity_frame", Time.frameCount); set("time", Time.time); set("fixed_time", Time.fixedTime); set("physics_steps", steps);
            set("scene", actor.gameObject.scene.name); set("hierarchy", Hierarchy(actor.transform)); set("instance_id", actor.GetInstanceID()); set("controller", actor.GetType().Name);
            set("active", actor.gameObject.activeInHierarchy); set("enabled", ((Behaviour)actor).enabled);
            HealthManager health = actor.GetComponent<HealthManager>(); set("hp", read(health, "hp")); set("dead", read(health, "isDead"));
            Vector3 p = actor.transform.position; set("x", p.x); set("y", p.y); set("z", p.z); set("scale_x", actor.transform.localScale.x); set("rotation_z", actor.transform.eulerAngles.z);
            Rigidbody2D body = actor.GetComponent<Rigidbody2D>();
            if (body == null) missing.Add("Rigidbody2D:null_component");
            else { set("body_x", body.position.x); set("body_y", body.position.y); set("body_rotation", body.rotation); set("vx", body.linearVelocity.x); set("vy", body.linearVelocity.y); }
            // Avoid linking another retail assembly: the animator is already held by each controller.
            bool walker = actor.GetType().Name == "Walker";
            object animator = read(actor, walker ? "animator" : "anim");
            object clip = read(animator, "currentClip");
            if (clip != null) set("clip", read(clip, "name"));
            set("animation_previous_frame", read(animator, "previousFrame")); set("animation_clip_time", read(animator, "clipTime")); set("animation_state", read(animator, "state"));
            if (walker)
            {
                string[] fields = { "state", "currentFacing", "turningFacing", "stopReason", "walkTimeRemaining", "pauseTimeRemaining", "turnCooldownRemaining" };
                string[] keys = { "walker_state", "walker_facing", "walker_turning_facing", "walker_stop_reason", "walker_walk_remaining", "walker_pause_remaining", "walker_turn_cooldown" };
                for (int i = 0; i < fields.Length; i++) set(keys[i], read(actor, fields[i]));
                set("los_can_see_hero", read(read(actor, "lineOfSightDetector"), "canSeeHero"));
                set("alert_hero_in_range", read(read(actor, "alertRange"), "isHeroInRange"));
            }
            else
            {
                set("climber_direction", read(actor, "currentDirection")); set("climber_clockwise", read(actor, "clockwise"));
                int before = missing.Count; object handle = read(actor, "turnRoutine");
                if (missing.Count == before) set("climber_turn_handle_present", handle != null);
                object previous = read(actor, "previousPos"), turn = read(actor, "previousTurnPos");
                if (previous is Vector2) { set("climber_previous_x", ((Vector2)previous).x); set("climber_previous_y", ((Vector2)previous).y); }
                else if (previous is Vector3) { set("climber_previous_x", ((Vector3)previous).x); set("climber_previous_y", ((Vector3)previous).y); }
                if (turn is Vector2) { set("climber_previous_turn_x", ((Vector2)turn).x); set("climber_previous_turn_y", ((Vector2)turn).y); }
                else if (turn is Vector3) { set("climber_previous_turn_x", ((Vector3)turn).x); set("climber_previous_turn_y", ((Vector3)turn).y); }
                set("climber_phase", "not_exposed");
            }
            // FSM read-only state accessor; do not send events or sample action methods.
            List<string> states = new List<string>();
            foreach (PlayMakerFSM fsm in actor.GetComponents<PlayMakerFSM>()) if (fsm.FsmName == "Zombie Swipe") states.Add(fsm.ActiveStateName);
            set("zombie_swipe_state", String.Join("|", states.ToArray()));
            if (walker && states.Count != 1) missing.Add("Zombie Swipe:matching_fsm_count=" + states.Count);
            set("missing", String.Join(";", missing.ToArray()));
            for (int i = 0; i < row.Length; i++) row[i] = Csv(row[i]);
            writer.WriteLine(String.Join(",", row));
        }
        private static string Hierarchy(Transform transform)
        {
            var parts = new List<string>(); int depth = 0;
            for (Transform t = transform; t != null && depth++ < 64; t = t.parent) parts.Add(t.name.Replace("/", "\\/") + "[" + t.GetSiblingIndex() + "]");
            if (depth > 64) parts.Add("<truncated>"); parts.Reverse(); return String.Join("/", parts.ToArray());
        }
        private static string N(object value) { return value is float ? ((float)value).ToString("R", CultureInfo.InvariantCulture) : Convert.ToString(value, CultureInfo.InvariantCulture); }
        private static string Csv(string value) { return "\"" + (value ?? "").Replace("\"", "\"\"") + "\""; }
        private static void Note(string text) { if (notices != null) { notices.WriteLine(text); notices.Flush(); } }
        private static void Loaded(Scene scene, LoadSceneMode mode) { dirty = true; }
        private static void Unloaded(Scene scene) { dirty = true; }
        public static void Dispose()
        {
            UnityEngine.SceneManagement.SceneManager.sceneLoaded -= Loaded; UnityEngine.SceneManagement.SceneManager.sceneUnloaded -= Unloaded;
            if (writer != null) { writer.Dispose(); writer = null; } if (notices != null) { notices.Dispose(); notices = null; }
            Actors.Clear(); Fields.Clear(); Removed.Clear();
        }
    }
}
