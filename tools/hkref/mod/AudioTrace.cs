// Managed call-site tracing only: native Unity audio calls still execute once.
using System;
using System.Globalization;
using System.IO;
using System.Collections.Generic;
using System.Reflection;
using UnityEngine;
using UnityEngine.Audio;

namespace HKReference
{
    public static class AudioTrace
    {
        private static StreamWriter writer;
        private static bool failed;
        private static long sequence;
        private static string outputDirectory;
        public static void Initialize(string output)
        {
            Dispose(); failed = false; sequence = 0; outputDirectory = output;
            writer = new StreamWriter(Path.Combine(output, "audio-calls.csv"), false);
            writer.AutoFlush = true;
            writer.WriteLine("sequence,queued_test_frame,unity_frame,time,fixed_time,input_tick,input_updates,operation,callsite,scene,hierarchy,source_id,clip_or_snapshot,asset_id,pitch,source_volume,volume_scale,x,y,z,parameter,value,phase");
        }
        public static void Dispose() { if (writer != null) { writer.Dispose(); writer = null; } }

        public static void DescribeHero(object hero)
        {
            // Runtime names/IDs are linkage evidence, not serialized PathIDs.
            if (hero == null || outputDirectory == null) return;
            using (StreamWriter metadata = new StreamWriter(Path.Combine(outputDirectory, "hero-audio-config.csv"), false))
            {
                metadata.WriteLine("field,type,object_name,instance_id,clip_name,clip_instance_id,samples,frequency");
                foreach (FieldInfo field in hero.GetType().GetFields(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance))
                {
                    string lower = field.Name.ToLowerInvariant();
                    if (!lower.Contains("footstep") && !lower.Contains("land")) continue;
                    UnityEngine.Object value = field.GetValue(hero) as UnityEngine.Object;
                    if (value == null) continue;
                    AudioSource source = value as AudioSource;
                    AudioClip clip = source != null ? source.clip : value as AudioClip;
                    string[] row = { field.Name, field.FieldType.FullName, value.name, N(value.GetInstanceID()),
                        clip == null ? "" : clip.name, clip == null ? "" : N(clip.GetInstanceID()),
                        clip == null ? "" : N(clip.samples), clip == null ? "" : N(clip.frequency) };
                    for (int i = 0; i < row.Length; i++) row[i] = Escape(row[i]);
                    metadata.WriteLine(String.Join(",", row));
                }
            }
        }

        public static void Play(AudioSource source, string site)
        { Record("Play", source, null, 1f, null, "", 0f, site); source.Play(); }
        public static void PlayOneShot(AudioSource source, AudioClip clip, string site)
        { Record("PlayOneShot", source, clip, 1f, null, "", 0f, site); source.PlayOneShot(clip); }
        public static void PlayOneShot(AudioSource source, AudioClip clip, float scale, string site)
        { Record("PlayOneShot", source, clip, scale, null, "", 0f, site); source.PlayOneShot(clip, scale); }
        public static void Stop(AudioSource source, string site)
        { Record("Stop", source, null, 1f, null, "", 0f, site); source.Stop(); }
        public static void Pause(AudioSource source, string site)
        { Record("Pause", source, null, 1f, null, "", 0f, site); source.Pause(); }
        public static void UnPause(AudioSource source, string site)
        { Record("UnPause", source, null, 1f, null, "", 0f, site); source.UnPause(); }
        public static void PlayClipAtPoint(AudioClip clip, Vector3 position, float volume, string site)
        { Record("PlayClipAtPoint", null, clip, volume, null, "", 0f, site, position); AudioSource.PlayClipAtPoint(clip, position, volume); }
        public static void TransitionTo(AudioMixerSnapshot snapshot, float duration, string site)
        { Record("TransitionTo", null, null, 1f, snapshot, "duration", duration, site); snapshot.TransitionTo(duration); }
        public static bool SetFloat(AudioMixer mixer, string parameter, float value, string site)
        { Record("SetFloat", null, null, 1f, mixer, parameter, value, site); return mixer.SetFloat(parameter, value); }

        private static void Record(string operation, AudioSource source, AudioClip suppliedClip, float scale,
            UnityEngine.Object asset, string parameter, float value, string site, Vector3? point = null)
        {
            // Diagnostic failures must not suppress, duplicate, or replace the
            // original call (including its own null/invalid-object exception).
            if (writer == null || failed) return;
            try
            {
                bool hasSource = source != null;
                AudioClip clip = operation == "PlayOneShot" || operation == "PlayClipAtPoint"
                    ? suppliedClip : hasSource ? source.clip : null;
                UnityEngine.Object named = asset != null ? asset : clip;
                Vector3 position = point.HasValue ? point.Value : hasSource ? source.transform.position : Vector3.zero;
                string[] row = {
                    N(sequence++), N(ReplayDevice.TestFrame), N(Time.frameCount), N(Time.time), N(Time.fixedTime),
                    N(ReplayDevice.LastUpdateTick), N(ReplayDevice.UpdateCount), operation, site,
                    hasSource ? source.gameObject.scene.name : UnityEngine.SceneManagement.SceneManager.GetActiveScene().name,
                    hasSource ? Hierarchy(source.transform) : "", hasSource ? N(source.GetInstanceID()) : "",
                    named == null ? "" : named.name, named == null ? "" : N(named.GetInstanceID()),
                    hasSource ? N(source.pitch) : "", hasSource ? N(source.volume) : "", N(scale),
                    hasSource || point.HasValue ? N(position.x) : "", hasSource || point.HasValue ? N(position.y) : "",
                    hasSource || point.HasValue ? N(position.z) : "", parameter, parameter.Length == 0 ? "" : N(value), "request"
                };
                for (int i = 0; i < row.Length; i++) row[i] = Escape(row[i]);
                writer.WriteLine(String.Join(",", row));
            }
            catch (Exception error)
            {
                failed = true;
                Debug.LogError("HKReference AudioTrace logging failed: " + error);
            }
        }
        private static string Hierarchy(Transform transform)
        {
            List<string> parts = new List<string>();
            for (Transform t = transform; t != null; t = t.parent)
                parts.Add(t.name.Replace("\\", "\\\\").Replace("/", "\\/") + "[" + N(t.GetSiblingIndex()) + "]");
            parts.Reverse(); return String.Join("/", parts.ToArray());
        }
        private static string N(object value)
        {
            if (value is float) return ((float)value).ToString("R", CultureInfo.InvariantCulture);
            return Convert.ToString(value, CultureInfo.InvariantCulture);
        }
        private static string Escape(string value)
        {
            value = value ?? "";
            return value.IndexOfAny(new char[] { ',', '"', '\r', '\n' }) < 0 ? value : "\"" + value.Replace("\"", "\"\"") + "\"";
        }
    }
}
