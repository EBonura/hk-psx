using Mono.Cecil;
using Mono.Cecil.Cil;
using System.Security.Cryptography;
using System.Text.Json;

if (args.Length != 4) throw new ArgumentException("source-dll copied-managed-dir driver-dll isolated-save-dir");
var source = Path.GetFullPath(args[0]);
var managed = Path.GetFullPath(args[1]);
var destination = Path.Combine(managed, "Assembly-CSharp.dll");
if (source == destination || !managed.Contains(Path.DirectorySeparatorChar + ".hkpsx" + Path.DirectorySeparatorChar))
    throw new ArgumentException("Only an isolated .hkpsx copy may be patched");
var resolver = new DefaultAssemblyResolver();
resolver.AddSearchDirectory(managed);
using var original = AssemblyDefinition.ReadAssembly(source, new ReaderParameters { AssemblyResolver = resolver });
using var driver = AssemblyDefinition.ReadAssembly(args[2], new ReaderParameters { AssemblyResolver = resolver });
var install = driver.MainModule.GetType("HKReference.Driver").Methods.Single(m => m.Name == "Install" && !m.HasParameters);
var manager = original.MainModule.GetType("GameManager");
var awake = manager.Methods.Single(m => m.Name == "Awake" && !m.HasParameters);
var imported = original.MainModule.ImportReference(install);
var processor = awake.Body.GetILProcessor();
int exits = 0, paths = 0;
foreach (var ret in awake.Body.Instructions.Where(i => i.OpCode == OpCodes.Ret).ToArray()) {
    // Keep any existing branch targets pointing at the new call.
    ret.OpCode = OpCodes.Call; ret.Operand = imported;
    processor.InsertAfter(ret, processor.Create(OpCodes.Ret)); exits++;
}
// StartManager belongs to Pre_Menu_Intro, before Menu_Title's GameManager.
// Install diagnostics there as well; Driver.Install itself is idempotent.
var startManager = original.MainModule.GetType("StartManager") ?? throw new InvalidOperationException("Missing StartManager");
var earlyAwake = startManager.Methods.Single(m => m.Name == "Awake" && !m.HasParameters);
if (!earlyAwake.HasBody || earlyAwake.Body.Instructions.Count(i => i.OpCode == OpCodes.Call
        && i.Operand is MethodReference m && m.Name == "get_platform") != 1
    || earlyAwake.Body.Instructions.Count(i => i.OpCode == OpCodes.Stfld
        && i.Operand is FieldReference f && f.DeclaringType.FullName == "StartManager" && f.Name == "platform") != 1
    || earlyAwake.Body.Instructions.Count(i => i.OpCode == OpCodes.Ret) != 1)
    throw new InvalidOperationException("StartManager.Awake source contract changed");
int earlyExits = 0;
var earlyProcessor = earlyAwake.Body.GetILProcessor();
foreach (var ret in earlyAwake.Body.Instructions.Where(i => i.OpCode == OpCodes.Ret).ToArray()) {
    ret.OpCode = OpCodes.Call; ret.Operand = imported;
    earlyProcessor.InsertAfter(ret, earlyProcessor.Create(OpCodes.Ret)); earlyExits++;
}

// Cecil retains short branch forms. Widen them in modified methods so inserted
// bytes cannot silently move an otherwise valid short target out of range.
void WidenBranches(MethodDefinition method) {
    var forms = new Dictionary<OpCode, OpCode> {
        [OpCodes.Br_S]=OpCodes.Br, [OpCodes.Brfalse_S]=OpCodes.Brfalse, [OpCodes.Brtrue_S]=OpCodes.Brtrue,
        [OpCodes.Beq_S]=OpCodes.Beq, [OpCodes.Bne_Un_S]=OpCodes.Bne_Un,
        [OpCodes.Bge_S]=OpCodes.Bge, [OpCodes.Bge_Un_S]=OpCodes.Bge_Un,
        [OpCodes.Bgt_S]=OpCodes.Bgt, [OpCodes.Bgt_Un_S]=OpCodes.Bgt_Un,
        [OpCodes.Ble_S]=OpCodes.Ble, [OpCodes.Ble_Un_S]=OpCodes.Ble_Un,
        [OpCodes.Blt_S]=OpCodes.Blt, [OpCodes.Blt_Un_S]=OpCodes.Blt_Un, [OpCodes.Leave_S]=OpCodes.Leave
    };
    foreach (var instruction in method.Body.Instructions)
        if (forms.TryGetValue(instruction.OpCode, out var wide)) instruction.OpCode = wide;
}
WidenBranches(awake); WidenBranches(earlyAwake);
IEnumerable<TypeDefinition> Types(IEnumerable<TypeDefinition> roots) {
    foreach (var type in roots) {yield return type; foreach (var child in Types(type.NestedTypes)) yield return child;}
}
foreach (var method in Types(original.MainModule.Types).SelectMany(t => t.Methods).Where(m => m.HasBody)) {
    foreach (var instruction in method.Body.Instructions) {
        if (instruction.Operand is MethodReference called && called.DeclaringType.FullName == "UnityEngine.Application" && called.Name == "get_persistentDataPath") {
            instruction.OpCode = OpCodes.Ldstr; instruction.Operand = args[3]; paths++;
        }
    }
}
if (exits == 0 || paths == 0) throw new InvalidOperationException("Expected bootstrap/path hooks not found");
// Replace only exact managed audio signatures. The wrapper consumes the same
// receiver/arguments plus a provenance string and invokes the native API once.
// Keep the original instruction object so branches/EH boundaries remain valid.
var audioType = driver.MainModule.GetType("HKReference.AudioTrace") ?? throw new InvalidOperationException("Missing AudioTrace");
var audioWrappers = audioType.Methods.Where(m => m.IsPublic && m.IsStatic && m.HasParameters && m.Parameters.Last().Name == "site").ToArray();
var audioSites = new List<object>();
var uncoveredAudio = new Dictionary<string, int>();
var audioCounts = new Dictionary<string, int>();
foreach (var method in Types(original.MainModule.Types).SelectMany(t => t.Methods).Where(m => m.HasBody)) {
    bool changed = false;
    foreach (var instruction in method.Body.Instructions.ToArray()) {
        if (instruction.Operand is not MethodReference called || !new[] {
            "UnityEngine.AudioSource", "UnityEngine.Audio.AudioMixer", "UnityEngine.Audio.AudioMixerSnapshot"
        }.Contains(called.DeclaringType.FullName)) continue;
        var parameters = called.Parameters.Select(p => p.ParameterType.FullName).ToList();
        if (called.HasThis) parameters.Insert(0, called.DeclaringType.FullName);
        parameters.Add("System.String");
        var candidates = audioWrappers.Where(w => w.Name == called.Name
            && w.ReturnType.FullName == called.ReturnType.FullName
            && w.Parameters.Select(p => p.ParameterType.FullName).SequenceEqual(parameters)).ToArray();
        if (candidates.Length == 0) {
            uncoveredAudio[called.FullName] = uncoveredAudio.GetValueOrDefault(called.FullName) + 1;
            continue;
        }
        if (candidates.Length != 1 || (instruction.OpCode != OpCodes.Call && instruction.OpCode != OpCodes.Callvirt)
            || instruction.Previous?.OpCode.OpCodeType == OpCodeType.Prefix || called.HasGenericParameters)
            throw new InvalidOperationException("Unsupported audio call contract: " + method.FullName + " -> " + called.FullName);
        string site = method.FullName + "@IL_" + instruction.Offset.ToString("x4");
        audioSites.Add(new { site, target = called.FullName, opcode = instruction.OpCode.Name, wrapper = candidates[0].FullName });
        audioCounts[called.FullName] = audioCounts.GetValueOrDefault(called.FullName) + 1;
        instruction.OpCode = OpCodes.Ldstr;
        instruction.Operand = site;
        method.Body.GetILProcessor().InsertAfter(instruction,
            Instruction.Create(OpCodes.Call, original.MainModule.ImportReference(candidates[0])));
        changed = true;
    }
    if (changed) WidenBranches(method);
}
foreach (string required in new[] { "Play", "PlayOneShot", "Stop", "TransitionTo" })
    if (!audioSites.Any() || !audioCounts.Keys.Any(k => k.Contains("::" + required + "(")))
        throw new InvalidOperationException("Expected original audio call missing: " + required);
original.Write(destination);
File.Copy(args[2], Path.Combine(managed, Path.GetFileName(args[2])), true);
Console.WriteLine(JsonSerializer.Serialize(new { source_sha256 = Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(source))).ToLowerInvariant(), patched_sha256 = Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(destination))).ToLowerInvariant(), awake_exits = exits, start_manager_awake_exits = earlyExits,
    isolated_path_calls = paths, audio_call_counts = audioCounts, audio_call_sites = audioSites,
    untraced_audio_references = uncoveredAudio,
    audio_scope = "Exact calls in original Assembly-CSharp only; wrappers retain native calls. No reflection/delegate/native/other-assembly calls, playOnAwake, or audible-output coverage." }));
