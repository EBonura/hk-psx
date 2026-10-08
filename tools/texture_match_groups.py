"""Exhaustive static-art comparisons and direct-representative review groups."""
import collections
import numpy as np
from audit_texture_similarity import composite,metrics,normalized


def score_matrix(items):
    """Same-size inputs; exact integer RGB sums with float64 matrix products.

    Outside the foreground union both composites equal the same background,
    so their squared difference is zero. Counting the union separately gives
    the same metric as the scalar oracle without per-pair image allocations.
    """
    mask=np.stack([(t['words']!=0).ravel()for t in items]).astype(np.float64)
    area=mask.sum(axis=1);union=area[:,None]+area[None,:]-mask@mask.T
    worst=np.zeros_like(union)
    for bg in (0,12,31):
        rgb=np.stack([composite(t,bg).ravel()for t in items]).astype(np.float64)
        norm=np.einsum('ij,ij->i',rgb,rgb)
        square=np.maximum(0,norm[:,None]+norm[None,:]-2*(rgb@rgb.T))
        worst=np.maximum(worst,square/(3*np.maximum(1,union)))
    return 100*(1-np.sqrt(worst)/31)


def eligible(t):
    return not t['stream'] and not t['black'] and len(t['modes'])==1 and not np.all((t['words']&32767)<=1)


def all_static_matches(items,threshold=80):
    groups=collections.defaultdict(list);normal={t['id']:normalized(t)for t in items if eligible(t)}
    for t in items:
        if eligible(t):groups[t['modes']].append(t)
    pairs=[];compared=0
    for mode,group in groups.items():
        score=score_matrix([normal[t['id']]for t in group]);dimensions=collections.defaultdict(list)
        for i,t in enumerate(group):dimensions[t['w'],t['h']].append(i)
        for indices in dimensions.values():
            if len(indices)>1:score[np.ix_(indices,indices)]=score_matrix([group[i]for i in indices])
        compared+=len(group)*(len(group)-1)//2
        for i,j in zip(*np.where(np.triu(score>=threshold,1))):
            a,b=group[i],group[j];same=(a['w'],a['h'])==(b['w'],b['h'])
            m=metrics(a,b)if same else metrics(normal[a['id']],normal[b['id']])
            if abs(m['similarity']-score[i,j])>1e-5:raise ValueError(f'Matrix/scalar mismatch: {m["similarity"]} vs {score[i,j]}')
            pairs.append(dict(a=a['id'],b=b['id'],comparison='native'if same else'normalized',same_source=bool(a.get('sprites',set())&b.get('sprites',set())),**m))
    pairs.sort(key=lambda p:(-p['similarity'],p['a'],p['b']))
    return dict(threshold=threshold,eligible_textures=len(normal),compared_pairs=compared,candidates=pairs,scope='All eligible static texture pairs of the same material, including different original sprites. Animation and all-black masks excluded. Native pixels for equal dimensions;48x48 nearest normalization otherwise.')


def review_groups(pairs,items,threshold):
    """Choose largest direct-match group, then repeat among remaining textures.

    This is a deterministic greedy review layout, not a globally optimal cover.
    Every member directly matches its final representative at the threshold;
    a chain of pairwise matches never silently becomes an admitted group.
    """
    adjacent=collections.defaultdict(dict)
    for p in pairs:
        if p['similarity']>=threshold:
            adjacent[p['a']][p['b']]=p['similarity'];adjacent[p['b']][p['a']]=p['similarity']
    remaining=set(adjacent);groups=[];area={t['id']:t['w']*t['h']for t in items}
    while remaining:
        rep=max(remaining,key=lambda i:(len(remaining&adjacent[i].keys()),area[i],-i))
        members=sorted(remaining&adjacent[rep].keys(),key=lambda i:(-adjacent[rep][i],i))
        groups.append(dict(representative=rep,members=[dict(id=rep,similarity=100)]+[dict(id=i,similarity=adjacent[rep][i])for i in members],minimum_similarity=min([adjacent[rep][i]for i in members],default=100)))
        remaining.difference_update([rep]+members)
    return groups
