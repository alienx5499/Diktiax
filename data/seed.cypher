// ==============================================================================
// Diktiax - FalkorDB seed graph
// ------------------------------------------------------------------------------
// Executed by FalkorGraphRepository::seed. Statements are separated by ';' and
// full-line '//' comments are stripped before execution. Indexes are created by
// the repository (see backend/src/db/queries.rs), not here.
//
// Graph model (exactly what the detectors traverse and ingest_claim MERGEs):
//   (Customer)-[:FILED_CLAIM]->(Claim)
//   (Customer)-[:HAS_PHONE]->(Phone {number})
//   (Customer)-[:LIVES_AT]->(Address {line})
//   (Customer)-[:OWNS]->(Vehicle)-[:HAS_VIN]->(VIN {vin})
//   (Vehicle)-[:INVOLVED_IN]->(Accident)-[:OCCURRED_AT]->(Location)
//   (Accident)-[:TRIGGERED_CLAIM]->(Claim)
//   (Claim)-[:REPAIRED_AT]->(RepairShop)
//   (Claim)-[:TREATED_AT]->(Clinic)
//   (Claim)-[:ASSIGNED_TO]->(Adjuster)
//   (RepairShop)-[:COLLUDES_WITH]->(Clinic)
//   (Claim)-[:ASSIGNED_INVESTIGATOR]->(Investigator)   (written by the agent's actions)
// Claim.filed_day = days since 1970-01-01 (used for time-window detection).
// ==============================================================================

CREATE
// ---------------------------------------------------------------- reference data
  (loc441:Location {id: "LOC-441", name: "5th Ave & Pine St, Oakland", address: "Intersection of 5th Ave & Pine St", city: "Oakland", state: "CA", traffic_camera: false}),
  (loc210:Location {id: "LOC-210", name: "Industrial Pkwy & Depot Way, San Jose", address: "Industrial Pkwy & Depot Way", city: "San Jose", state: "CA", traffic_camera: false}),
  (loc101:Location {id: "LOC-101", name: "Market St & 3rd St, San Francisco", address: "Market St & 3rd St", city: "San Francisco", state: "CA", traffic_camera: true}),
  (loc305:Location {id: "LOC-305", name: "University Ave & Shattuck, Berkeley", address: "University Ave & Shattuck Ave", city: "Berkeley", state: "CA", traffic_camera: true}),

  (shop801:RepairShop {id: "SHOP-801", name: "Apex Auto Collision & Restoration", city: "Oakland", certified: false}),
  (shop505:RepairShop {id: "SHOP-505", name: "Golden Bay Precision Auto", city: "San Jose", certified: false}),
  (shop100:RepairShop {id: "SHOP-100", name: "Certified Dealership Body Works", city: "San Francisco", certified: true}),
  (shop220:RepairShop {id: "SHOP-220", name: "Berkeley Honest Auto Body", city: "Berkeley", certified: true}),

  (med201:Clinic {id: "MED-201", name: "Golden State Wellness & Spine Care", specialty: "Chiropractic & Soft Tissue Rehab"}),
  (med100:Clinic {id: "MED-100", name: "St. Jude Regional Medical Center", specialty: "Emergency Medicine"}),

  (adj01:Adjuster {id: "ADJ-01", name: "Sarah Chen", role: "Senior SIU Fraud Specialist"}),
  (adj04:Adjuster {id: "ADJ-04", name: "David Miller", role: "General Claims Adjuster"}),
  (adj09:Adjuster {id: "ADJ-09", name: "Robert Morales", role: "Auto Physical Damage Specialist"}),

  (shop801)-[:COLLUDES_WITH {referral_fee_pct: 15.0, established: "2024-03-01"}]->(med201),

// ---------------------------------------------------------------- shared contact points
  (ph8821:Phone {number: "+1-555-019-8821"}),
  (ph4410:Phone {number: "+1-555-019-4410"}),
  (ph9912:Phone {number: "+1-555-014-9912"}),
  (ph3344:Phone {number: "+1-555-081-3344"}),
  (ph7781:Phone {number: "+1-555-062-7781"}),
  (ph2290:Phone {number: "+1-555-047-2290"}),

  (adSpring:Address {line: "742 Evergreen Terrace, Springfield, CA"}),
  (adHayward:Address {line: "108 Industrial Way, Hayward, CA"}),
  (adReno:Address {line: "404 Mission St, Reno, NV"}),
  (adOxford:Address {line: "2120 Oxford St, Berkeley, CA"}),
  (adGrove:Address {line: "55 Grove St, San Francisco, CA"}),
  (adElm:Address {line: "910 Elm Ave, Palo Alto, CA"}),

// ---------------------------------------------------------------- SYNDICATE 1: ghost collision ring
// Three staged crashes at LOC-441, all repaired at SHOP-801, treated at MED-201,
// handled by ADJ-04, no police reports. Victor & Dmitri share a burner phone,
// Dmitri & Lena share an address.
  (cust1001:Customer {id: "CUST-1001", name: "Victor Vance"}),
  (cust1002:Customer {id: "CUST-1002", name: "Dmitri Volkov"}),
  (cust1005:Customer {id: "CUST-1005", name: "Lena Sorokina"}),
  (cust1001)-[:HAS_PHONE]->(ph8821), (cust1001)-[:LIVES_AT]->(adSpring),
  (cust1002)-[:HAS_PHONE]->(ph8821), (cust1002)-[:LIVES_AT]->(adHayward),
  (cust1005)-[:HAS_PHONE]->(ph4410), (cust1005)-[:LIVES_AT]->(adHayward),

  (veh1001:Vehicle {id: "VEH-1001", make: "Honda", model: "Accord EX-L", year: 2021, plate: "7XYZ89", jurisdiction: "CA"}),
  (veh1002:Vehicle {id: "VEH-1002", make: "Toyota", model: "Camry XSE", year: 2020, plate: "8ABC44", jurisdiction: "CA"}),
  (veh1005:Vehicle {id: "VEH-1005", make: "Tesla", model: "Model 3", year: 2019, plate: "8TSL551", jurisdiction: "CA"}),
  (veh1001)-[:HAS_VIN]->(:VIN {vin: "1HGCR2F83HA001294"}),
  (veh1002)-[:HAS_VIN]->(:VIN {vin: "4T1B11HK5JU889102"}),
  (veh1005)-[:HAS_VIN]->(:VIN {vin: "5YJ3E1EA7KF317562"}),
  (cust1001)-[:OWNS]->(veh1001), (cust1002)-[:OWNS]->(veh1002), (cust1005)-[:OWNS]->(veh1005),

  (acc081:Accident {id: "ACC-081", timestamp: "2026-09-14T23:15:00Z", description: "Nighttime side-impact at blind intersection. Opposing driver fled.", police_report_filed: false}),
  (acc042:Accident {id: "ACC-042", timestamp: "2026-08-22T22:45:00Z", description: "Late evening T-bone during lane merge. Phantom vehicle left scene.", police_report_filed: false}),
  (acc067:Accident {id: "ACC-067", timestamp: "2026-09-01T23:40:00Z", description: "Sudden-stop rear impact. Lead vehicle drove away.", police_report_filed: false}),
  (veh1001)-[:INVOLVED_IN]->(acc081), (acc081)-[:OCCURRED_AT]->(loc441),
  (veh1002)-[:INVOLVED_IN]->(acc042), (acc042)-[:OCCURRED_AT]->(loc441),
  (veh1005)-[:INVOLVED_IN]->(acc067), (acc067)-[:OCCURRED_AT]->(loc441),

  (clm48291:Claim {id: "CLM-48291", date_filed: "2026-09-16T10:00:00Z", filed_day: 20712, amount_claimed: 42500.0, damage_type: "COLLISION_AND_INJURY", status: "SUBMITTED"}),
  (clm39102:Claim {id: "CLM-39102", date_filed: "2026-08-24T09:30:00Z", filed_day: 20689, amount_claimed: 38000.0, damage_type: "COLLISION_AND_INJURY", status: "APPROVED"}),
  (clm44730:Claim {id: "CLM-44730", date_filed: "2026-09-03T08:15:00Z", filed_day: 20699, amount_claimed: 35500.0, damage_type: "COLLISION_AND_INJURY", status: "APPROVED"}),
  (cust1001)-[:FILED_CLAIM]->(clm48291), (acc081)-[:TRIGGERED_CLAIM]->(clm48291),
  (cust1002)-[:FILED_CLAIM]->(clm39102), (acc042)-[:TRIGGERED_CLAIM]->(clm39102),
  (cust1005)-[:FILED_CLAIM]->(clm44730), (acc067)-[:TRIGGERED_CLAIM]->(clm44730),
  (clm48291)-[:REPAIRED_AT {quote_amount: 24000.0}]->(shop801),
  (clm39102)-[:REPAIRED_AT {quote_amount: 21500.0}]->(shop801),
  (clm44730)-[:REPAIRED_AT {quote_amount: 19800.0}]->(shop801),
  (clm48291)-[:TREATED_AT {billed_amount: 18500.0}]->(med201),
  (clm39102)-[:TREATED_AT {billed_amount: 16500.0}]->(med201),
  (clm44730)-[:TREATED_AT {billed_amount: 15700.0}]->(med201),
  (clm48291)-[:ASSIGNED_TO]->(adj04),
  (clm39102)-[:ASSIGNED_TO]->(adj04),
  (clm44730)-[:ASSIGNED_TO]->(adj04),

// ---------------------------------------------------------------- SYNDICATE 2: VIN cloning ring
// One VIN registered in CA and NV, two total-loss claims; synthetic identities
// share phone and address.
  (cust1003:Customer {id: "CUST-1003", name: "Marcus Holloway"}),
  (cust1004:Customer {id: "CUST-1004", name: "Mark H. Vance"}),
  (cust1003)-[:HAS_PHONE]->(ph9912), (cust1003)-[:LIVES_AT]->(adReno),
  (cust1004)-[:HAS_PHONE]->(ph9912), (cust1004)-[:LIVES_AT]->(adReno),

  (vin88102:VIN {vin: "1FTFW1ED4MFA88102"}),
  (vehCA:Vehicle {id: "VEH-88102-CA", make: "Ford", model: "F-150 Raptor", year: 2022, plate: "RAPTOR1", jurisdiction: "CA"}),
  (vehNV:Vehicle {id: "VEH-88102-NV", make: "Ford", model: "F-150 Raptor", year: 2022, plate: "NV-9921", jurisdiction: "NV"}),
  (vehCA)-[:HAS_VIN]->(vin88102), (vehNV)-[:HAS_VIN]->(vin88102),
  (cust1003)-[:OWNS]->(vehCA), (cust1004)-[:OWNS]->(vehNV),

  (acc099:Accident {id: "ACC-099", timestamp: "2026-09-20T03:10:00Z", description: "Single-vehicle rollover on rural curve. Vehicle caught fire.", police_report_filed: false}),
  (acc033:Accident {id: "ACC-033", timestamp: "2026-07-15T02:00:00Z", description: "Reported theft and burnout in desert basin.", police_report_filed: false}),
  (vehCA)-[:INVOLVED_IN]->(acc099), (acc099)-[:OCCURRED_AT]->(loc210),
  (vehNV)-[:INVOLVED_IN]->(acc033), (acc033)-[:OCCURRED_AT]->(loc210),

  (clm55104:Claim {id: "CLM-55104", date_filed: "2026-09-21T08:00:00Z", filed_day: 20717, amount_claimed: 58000.0, damage_type: "TOTAL_LOSS_FIRE", status: "SUBMITTED"}),
  (clm31205:Claim {id: "CLM-31205", date_filed: "2026-07-18T11:00:00Z", filed_day: 20652, amount_claimed: 56000.0, damage_type: "THEFT_TOTAL_LOSS", status: "APPROVED"}),
  (cust1003)-[:FILED_CLAIM]->(clm55104), (acc099)-[:TRIGGERED_CLAIM]->(clm55104),
  (cust1004)-[:FILED_CLAIM]->(clm31205), (acc033)-[:TRIGGERED_CLAIM]->(clm31205),
  (clm55104)-[:REPAIRED_AT {quote_amount: 58000.0}]->(shop505),
  (clm55104)-[:ASSIGNED_TO]->(adj04),
  (clm31205)-[:ASSIGNED_TO]->(adj04),

// ---------------------------------------------------------------- clean control claims
// Police-verified, unique contacts and VINs, no shared location+shop pattern.
  (cust9001:Customer {id: "CUST-9001", name: "Alice Montgomery"}),
  (cust9002:Customer {id: "CUST-9002", name: "Priya Raman"}),
  (cust9003:Customer {id: "CUST-9003", name: "Tom Becker"}),
  (cust9001)-[:HAS_PHONE]->(ph3344), (cust9001)-[:LIVES_AT]->(adOxford),
  (cust9002)-[:HAS_PHONE]->(ph7781), (cust9002)-[:LIVES_AT]->(adGrove),
  (cust9003)-[:HAS_PHONE]->(ph2290), (cust9003)-[:LIVES_AT]->(adElm),

  (veh9001:Vehicle {id: "VEH-9001", make: "Audi", model: "Q5 2.0T", year: 2023, plate: "CAL-Q5", jurisdiction: "CA"}),
  (veh9002:Vehicle {id: "VEH-9002", make: "Toyota", model: "Corolla", year: 2020, plate: "8PRY221", jurisdiction: "CA"}),
  (veh9003:Vehicle {id: "VEH-9003", make: "Chevrolet", model: "Malibu", year: 2020, plate: "8TBK907", jurisdiction: "CA"}),
  (veh9001)-[:HAS_VIN]->(:VIN {vin: "WAUZZZF28MA091823"}),
  (veh9002)-[:HAS_VIN]->(:VIN {vin: "JTDKARFU0L3104218"}),
  (veh9003)-[:HAS_VIN]->(:VIN {vin: "1G1ZD5ST8LF052310"}),
  (cust9001)-[:OWNS]->(veh9001), (cust9002)-[:OWNS]->(veh9002), (cust9003)-[:OWNS]->(veh9003),

  (acc500:Accident {id: "ACC-500", timestamp: "2026-09-28T14:10:00Z", description: "Low-speed rear-end bump at signal. Police report filed.", police_report_filed: true}),
  (acc512:Accident {id: "ACC-512", timestamp: "2026-09-10T17:30:00Z", description: "Parking-lot side swipe, witness statement and police report.", police_report_filed: true}),
  (acc520:Accident {id: "ACC-520", timestamp: "2026-10-02T08:05:00Z", description: "Minor fender bender in rush hour, camera footage available.", police_report_filed: true}),
  (veh9001)-[:INVOLVED_IN]->(acc500), (acc500)-[:OCCURRED_AT]->(loc101),
  (veh9002)-[:INVOLVED_IN]->(acc512), (acc512)-[:OCCURRED_AT]->(loc305),
  (veh9003)-[:INVOLVED_IN]->(acc520), (acc520)-[:OCCURRED_AT]->(loc101),

  (clm10022:Claim {id: "CLM-10022", date_filed: "2026-09-29T11:20:00Z", filed_day: 20725, amount_claimed: 4200.0, damage_type: "REAR_BUMPER_REPAIR", status: "SUBMITTED"}),
  (clm20417:Claim {id: "CLM-20417", date_filed: "2026-09-11T09:00:00Z", filed_day: 20707, amount_claimed: 6800.0, damage_type: "SIDE_PANEL_REPAIR", status: "APPROVED"}),
  (clm20988:Claim {id: "CLM-20988", date_filed: "2026-10-03T10:45:00Z", filed_day: 20729, amount_claimed: 3100.0, damage_type: "FRONT_BUMPER_REPAIR", status: "SUBMITTED"}),
  (cust9001)-[:FILED_CLAIM]->(clm10022), (acc500)-[:TRIGGERED_CLAIM]->(clm10022),
  (cust9002)-[:FILED_CLAIM]->(clm20417), (acc512)-[:TRIGGERED_CLAIM]->(clm20417),
  (cust9003)-[:FILED_CLAIM]->(clm20988), (acc520)-[:TRIGGERED_CLAIM]->(clm20988),
  (clm10022)-[:REPAIRED_AT {quote_amount: 4200.0}]->(shop100),
  (clm20417)-[:REPAIRED_AT {quote_amount: 5100.0}]->(shop220),
  (clm20988)-[:REPAIRED_AT {quote_amount: 3100.0}]->(shop220),
  (clm20417)-[:TREATED_AT {billed_amount: 1700.0}]->(med100),
  (clm10022)-[:ASSIGNED_TO]->(adj09),
  (clm20417)-[:ASSIGNED_TO]->(adj01),
  (clm20988)-[:ASSIGNED_TO]->(adj09),

// ---------------------------------------------------------------- SIU investigator pool
// specialties hold detector pattern names; the agent picks the best coverage with
// the fewest open cases and increments open_cases on assignment.
  (:Investigator {id: "INV-201", name: "Elena Marsh", specialties: ["Ghost Collision Ring", "Adjuster-Shop-Clinic Kickback Triangle"], open_cases: 4}),
  (:Investigator {id: "INV-202", name: "Omar Haddad", specialties: ["Ghost Collision Ring", "Shared Contact Network"], open_cases: 2}),
  (:Investigator {id: "INV-203", name: "Grace Liu", specialties: ["Cloned VIN Across Jurisdictions"], open_cases: 1}),
  (:Investigator {id: "INV-204", name: "Daniel Okafor", specialties: ["Shared Contact Network", "Cloned VIN Across Jurisdictions"], open_cases: 5});
