"""Synthetic boundary tests; no retail assets required."""
import copy,sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'host'))
from focus_audio import SPU_BASE,assemble,oneshot_payload,validate_contract

def value(v):return {'useVariable':False,'value':v}
def target():return {'ownerOption':1,'gameObject':{'useVariable':True,'name':'Charge Audio'}}
def action(kind,**fields):return {'action':kind,'enabled':True,'fields':fields}
def fixture():
    states={s:[] for s in ('Focus Start','Focus Heal','Focus Cancel','Focus Get Finish','Regain Control','Cancel Some','FSM Cancel','Cancel All','Focus','Full HP?')}
    states['Focus Start']=[action('AudioPlay',gameObject=target(),volume=value(1),oneShotClip=value({'m_FileID':0,'m_PathID':0}))]
    states['Focus Heal']=[action('AudioPlayerOneShotSingle',volume=value(1),pitchMin=value(1),pitchMax=value(1),delay=value(0),audioClip=value({'m_FileID':0,'m_PathID':1260}),audioPlayer=value({'m_FileID':0,'m_PathID':4126}))]
    for s in ('Focus Cancel','Focus Get Finish'):states[s]=[action('FadeAudio',gameObject=target(),startVolume=value(1),endVolume=value(0),time=value(0.33000001311302185))]
    for s in ('Regain Control','Cancel Some','FSM Cancel','Cancel All'):states[s]=[action('AudioStop',gameObject=target())]
    return states,{'LEAVING SCENE':'Cancel Some','FSM CANCEL':'FSM Cancel','HERO DAMAGED':'Reset Cam Zoom'}

class FocusAudioTests(unittest.TestCase):
    def test_full_bank_flags_preserve_all_sample_blocks(self):
        charge=bytes([12,0])+bytes(14)+bytes([28,0])+bytes([0x31])*14
        heal=bytes([12,0])+bytes([0x12])*14
        bank,length=assemble(charge,heal,SPU_BASE)
        self.assertEqual(length,len(charge));self.assertEqual(len(bank),len(charge)+len(heal)+16)
        self.assertEqual(bank[1],4);self.assertEqual(bank[len(charge)-15],3)
        self.assertEqual(bank[2:16],charge[2:16]);self.assertEqual(bank[18:32],charge[18:32])
        self.assertEqual(bank[length:length+len(heal)],heal)
        self.assertEqual(bank[-16:],bytes([12,1])+bytes(14))

    def test_invalid_input_and_capacity_rejected(self):
        block=bytes([12,0])+bytes(14)
        for payload in (b'',bytes(15),bytes([0x5c,0])+bytes(14),bytes([12,1])+bytes(14)):
            with self.subTest(payload=payload),self.assertRaises(ValueError):oneshot_payload(payload)
        with self.assertRaises(ValueError):assemble(block,block,SPU_BASE+16)
        with self.assertRaises(ValueError):assemble(block*10000,block,SPU_BASE)

    def test_nominal_fade_and_interrupt_routes(self):
        states,globals_=fixture();self.assertEqual(validate_contract(states,globals_),20)
        for name in ('Focus Start','Focus Heal','Focus Cancel','Focus Get Finish','Regain Control','Cancel Some','FSM Cancel','Cancel All'):
            broken=copy.deepcopy(states);broken[name][0]['enabled']=False
            with self.subTest(state=name),self.assertRaises(ValueError):validate_contract(broken,globals_)
        broken=dict(globals_,**{'HERO DAMAGED':'Focus'})
        with self.assertRaises(ValueError):validate_contract(states,broken)

    def test_repeat_never_restarts_or_stops_charge(self):
        for kind in ('AudioPlay','AudioStop','FadeAudio'):
            states,globals_=fixture();states['Focus'].append(action(kind,gameObject=target()))
            with self.subTest(kind=kind),self.assertRaises(ValueError):validate_contract(states,globals_)

    def test_changed_clip_volume_pitch_and_fade_rejected(self):
        states,globals_=fixture()
        for state,key,replacement in [('Focus Start','volume',0),('Focus Heal','pitchMax',2),('Focus Heal','delay',0.1),('Focus Heal','audioClip',{'m_FileID':0,'m_PathID':42}),('Focus Cancel','time',0.5)]:
            broken=copy.deepcopy(states);broken[state][0]['fields'][key]=value(replacement)
            with self.subTest(key=key),self.assertRaises(ValueError):validate_contract(broken,globals_)

if __name__=='__main__':unittest.main()
