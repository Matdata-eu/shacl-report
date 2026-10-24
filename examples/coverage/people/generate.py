import random, sys
random.seed(7)
first = ["Ann","Bart","Chloé","Dirk","Els","Filip","Greet","Hugo","Inge","Jan","Katrien","Luc","Marie","Nico","Olga","Pieter","Rik","Sara","Tom","Vera"]
last = ["Peeters","Janssens","Maes","Jacobs","Willems","Claes","Goossens","Wouters","De Smet","Dubois"]
out = []
w = out.append
w("""@prefix xsd:  <http://www.w3.org/2001/XMLSchema#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix ex:   <http://example.org/people#> .
@prefix p:    <http://example.org/people/id/> .

# Generated demo data for the coverage report. ex:Employee is a subclass of
# ex:Person here in the data graph, so ex:PersonShape applies to employees.
ex:Employee rdfs:subClassOf ex:Person .
""")
def person(i, employee, org=None):
    n = f"{random.choice(first)} {random.choice(last)}"
    lines = [f"p:person{i:03d} a {'ex:Employee' if employee else 'ex:Person'} ;",
             f'    ex:name "{n}" ;',
             f'    rdfs:label "{n}"@en ;']
    if random.random() < 0.9:
        lines.append(f'    ex:birthDate "{random.randint(1950,2004)}-{random.randint(1,12):02d}-{random.randint(1,28):02d}"^^xsd:date ;')
    if random.random() < 0.8:
        lines.append(f'    ex:email "{n.split()[0].lower()}{i}@example.org" ;')
    if random.random() < 0.85:
        if random.random() < 0.5:
            lines.append(f'    ex:address [ a ex:Address ; ex:street "Kerkstraat {i}" ; ex:postalCode "{1000+i}" ; ex:city "Leuven" ; ex:country "BE" ] ;')
        else:
            lines.append(f'    ex:address p:address{i:03d} ;')
            out.append(f'p:address{i:03d} a ex:Address ; ex:street "Dorpsstraat {i}" ; ex:postalCode "{2000+i} AB" ; ex:city "Breda" ; ex:country "NL" ; ex:geo <http://example.org/geo/{i}> .')
    for c in range(random.randint(0,2)):
        lines.append(f'    ex:contactPoint [ ex:channel ex:{random.choice(["Phone","Email","Chat"])} ; ex:value "+32 470 {i:03d} {c}{c}{c}" ] ;')
    if random.random() < 0.6:
        lines.append(f"    ex:knows p:person{random.randint(0,99):03d} ;")
    if random.random() < 0.35:
        lines.append(f'    ex:nickname "{n.split()[0][:3]}" ;')
    if random.random() < 0.1:
        lines.append(f'    ex:favouriteColour "{random.choice(["green","blue","red"])}" ;')
    if employee:
        lines.append(f'    ex:employeeId "E{100000+i}" ;')
        lines.append(f'    ex:worksFor p:org{org} ;')
        lines.append(f'    ex:startDate "{random.randint(2000,2020)}-01-01"^^xsd:date ;')
        if random.random() < 0.2:
            lines.append(f'    ex:endDate "2025-06-30"^^xsd:date ;')
        lines.append(f'    ex:salary {random.randint(2500,7000)} ;')
        if random.random() < 0.7:
            lines.append(f'    ex:deskNumber "B2.{random.randint(100,199)}" ;')
    lines[-1] = lines[-1][:-2] + " ."
    w("\n".join(lines))
for i in range(40):
    person(i, False)
for i in range(40, 100):
    person(i, True, random.randint(1, 6))
sectors = ["ex:Public","ex:Private","ex:NonProfit"]
for o in range(1, 8):
    w(f'p:org{o} a ex:Organisation ; ex:legalName "Organisation {o} NV" ; ex:sector {random.choice(sectors)} ; ex:website <https://org{o}.example> ; rdfs:label "Organisation {o}"@en .')
for v in range(6):
    w(f'p:vehicle{v} a ex:Vehicle ; ex:plate "1-ABC-{v:03d}" ; ex:registeredOn "2019-0{v+1}-15"^^xsd:date ; ex:colour "white" ; rdfs:label "Van {v}" .')
for c in range(12):
    w(f'p:car{c} a ex:Car ; ex:plate "2-CAR-{c:03d}" ; ex:registeredOn "2021-0{c%9+1}-01"^^xsd:date ; ex:trunkVolume {300+c*10} .')
for n in range(20):
    w(f'p:invoice{n} a ex:Invoice ; ex:invoiceNumber "2026-{n:04d}" ; ex:amount {100+n*7}.50 ; ex:issuedTo p:org{n%7+1} ; ex:issueDate "2026-0{n%9+1}-10"^^xsd:date .')
w('<http://example.org/codes/BE> ex:code "BE" ; rdfs:label "Belgium"@en .')
w('<http://example.org/codes/NL> ex:code "NL" ; rdfs:label "Netherlands"@en .')
print("\n\n".join(out))
