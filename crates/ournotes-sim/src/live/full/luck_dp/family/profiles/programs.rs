//! Exact permutation transport of a completed profile; physical labels are never quotiented away.
use super::*;

/// A complete canonical projected input plus a bijection from one admitted physical profile. Canonical
/// equality deliberately excludes origin identity; constructing or transporting a proof still checks it.
#[derive(Debug)]
pub struct LuckFamilyProgramKey {
    origin: Arc<()>,
    mapping: Arc<DomainTerminalMapping>,
    profile: usize,
    physical_to_canonical: [usize; SLOTS],
    bytes: Vec<u8>,
}

impl LuckFamilyProgramKey {
    fn belongs_to(&self, domain: &LuckFamilyDomain, profile: usize) -> bool {
        Arc::ptr_eq(&self.origin, &domain.identity)
            && Arc::ptr_eq(&self.mapping, &domain.mapping)
            && self.profile == profile
    }
}

/// Every canonical physical-position permutation has one completed joint curve. This is a probability
/// proof, not a cached score, resource assignment, approximate curve class or partially prepared profile.
#[derive(Debug)]
pub struct LuckFamilyProgram {
    mapping: Arc<DomainTerminalMapping>,
    key: Vec<u8>,
    laws: Vec<Option<Arc<LuckDpCertifiedResult>>>,
    bytes: usize,
}

fn position_index(positions: &[usize; SLOTS]) -> Option<usize> {
    let mut used = 0u8;
    let mut index = 0usize;
    for (i, &position) in positions.iter().enumerate() {
        if position >= SLOTS || used & (1 << position) != 0 {
            return None;
        }
        used |= 1 << position;
        let smaller = positions[i + 1..].iter().filter(|&&other| other < position).count();
        index = index.checked_mul(SLOTS - i)?.checked_add(smaller)?;
    }
    (index < ORDERS).then_some(index)
}

fn unique_curve_bytes<'a>(curves: impl Iterator<Item = &'a Arc<LuckDpCertifiedResult>>) -> Option<usize> {
    let mut seen = [std::ptr::null(); ORDERS];
    let mut count = 0;
    let mut bytes = 0usize;
    for curve in curves {
        let pointer = Arc::as_ptr(curve);
        if !seen[..count].contains(&pointer) {
            *seen.get_mut(count)? = pointer;
            count += 1;
            bytes = bytes.checked_add(curve_bytes(curve).ok()?)?;
        }
    }
    Some(bytes)
}

impl LuckFamilyContext<'_> {
    /// Optional identity proof. Every selected source row must pass the existing closed character-unread
    /// proof, including omitted score rows. Every other complete Performer field and vector order is retained.
    pub fn profile_program_key(
        &self,
        domain: &LuckFamilyDomain,
        profile: usize,
        capacity: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Option<LuckFamilyProgramKey> {
        if cancelled() || capacity == 0 || !Arc::ptr_eq(&self.mapping, &domain.mapping) {
            return None;
        }
        let resources = domain.admitted.bindings.profiles.get(profile)?;
        let mut physical = domain.admitted.base.clone();
        for (owner, resource) in resources.iter().copied().enumerate() {
            if let Some(resource) = resource {
                physical[owner] =
                    domain.choices[owner].iter().find(|choice| choice.resource == Some(resource))?.performer.clone();
            }
        }
        let physical = physical.map(|performer| projected(self.master, &performer));
        let keys = input_reuse::InputKeys::new(self.master, &physical)?;
        let (bytes, physical_to_canonical) = keys.canonical(capacity)?;
        if cancelled() {
            return None;
        }
        Some(LuckFamilyProgramKey {
            origin: Arc::clone(&domain.identity),
            mapping: Arc::clone(&self.mapping),
            profile,
            physical_to_canonical,
            bytes,
        })
    }
}

impl LuckFamilyProgram {
    pub fn matches(&self, key: &LuckFamilyProgramKey) -> bool {
        Arc::ptr_eq(&self.mapping, &key.mapping) && self.key == key.bytes
    }

    /// Compile only from the very profile which authorized this key and bijection. A caller cannot combine
    /// key A with a completed donor B merely because both belong to one immutable context.
    pub fn from_profile(
        domain: &LuckFamilyDomain,
        key: LuckFamilyProgramKey,
        profile: &LuckFamilyProfile,
        capacity: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Option<Self> {
        if cancelled()
            || !domain.owns_profile(profile)
            || !key.belongs_to(domain, profile.profile)
            || profile.orders.len() != ORDERS
        {
            return None;
        }
        position_index(&key.physical_to_canonical)?;
        let mut laws = reserve(ORDERS).ok()?;
        laws.resize_with(ORDERS, || None);
        for law in &profile.orders {
            if cancelled() || law.profile != profile.profile {
                return None;
            }
            position_index(&law.positions)?;
            let mut positions = [0; SLOTS];
            for slot in 0..SLOTS {
                positions[key.physical_to_canonical[slot]] = law.positions[slot];
            }
            let place = laws.get_mut(position_index(&positions)?)?;
            if place.is_some() {
                return None;
            }
            *place = Some(Arc::clone(&law.probability));
        }
        if laws.iter().any(Option::is_none) || cancelled() {
            return None;
        }
        let bytes = size_of::<Self>()
            .checked_add(mapping_bytes(&key.mapping)?)?
            .checked_add(key.bytes.capacity())?
            .checked_add(laws.capacity().checked_mul(size_of::<Option<Arc<LuckDpCertifiedResult>>>())?)?
            .checked_add(unique_curve_bytes(laws.iter().flatten())?)?;
        if bytes > capacity {
            return None;
        }
        Some(Self { mapping: key.mapping, key: key.bytes, laws, bytes })
    }

    /// Rebuild the target's own labels in its original lexicographic order. The source array seen by native
    /// execution is identical after the fixed slot bijection; no writer or floating-point operation commutes.
    pub fn transport(
        &self,
        domain: &LuckFamilyDomain,
        key: &LuckFamilyProgramKey,
        profile: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Option<LuckFamilyProfile> {
        if cancelled() || !key.belongs_to(domain, profile) || !self.matches(key) || self.laws.len() != ORDERS {
            return None;
        }
        position_index(&key.physical_to_canonical)?;
        let mut orders = reserve(ORDERS).ok()?;
        let mut coverage = FamilyCoverage::new(1).ok()?;
        let mut order = [0, 1, 2, 3, 4];
        for ordinal in 0..ORDERS {
            if cancelled() {
                return None;
            }
            let mut positions = [0; SLOTS];
            for (position, &slot) in order.iter().enumerate() {
                positions[slot] = position;
            }
            let mut canonical = [0; SLOTS];
            for slot in 0..SLOTS {
                canonical[key.physical_to_canonical[slot]] = positions[slot];
            }
            let probability = Arc::clone(self.laws.get(position_index(&canonical)?)?.as_ref()?);
            if !coverage.mark(0, position_index(&positions)?) {
                return None;
            }
            orders.push(LuckFamilyOrderLaw { positions, profile, probability });
            if next_order(&mut order) != (ordinal + 1 < ORDERS) {
                return None;
            }
        }
        if !coverage.complete() || cancelled() {
            return None;
        }
        let bytes = size_of::<LuckFamilyProfile>()
            .checked_add(2 * size_of::<usize>())?
            .checked_add(mapping_bytes(&domain.mapping)?)?
            .checked_add(orders.capacity().checked_mul(size_of::<LuckFamilyOrderLaw>())?)?
            .checked_add(unique_curve_bytes(orders.iter().map(|law| &law.probability))?)?;
        if bytes > domain.limits.max_retained_bytes {
            return None;
        }
        Some(LuckFamilyProfile {
            identity: Arc::clone(&domain.identity),
            mapping: Arc::clone(&domain.mapping),
            profile,
            orders,
            bytes,
        })
    }

    /// Complete key, 120 Arc slots, common mapping and each distinct retained probability allocation.
    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }
}

impl LuckFamilyProgram {
    /// Exact retained payload of a bounded collection, counting common mapping and curve allocations once.
    /// The caller separately charges its entry-buffer capacity instead of these inline object sizes.
    pub fn retained_collection_bytes<'a>(programs: impl Iterator<Item = &'a Self>) -> Option<usize> {
        let mut mappings = [std::ptr::null(); 64];
        let mut curves = [std::ptr::null(); 64 * ORDERS];
        let mut mapping_count = 0;
        let mut curve_count = 0;
        let mut bytes = 0usize;
        for program in programs {
            bytes = bytes
                .checked_add(size_of::<Self>())?
                .checked_add(program.key.capacity())?
                .checked_add(program.laws.capacity().checked_mul(size_of::<Option<Arc<LuckDpCertifiedResult>>>())?)?;
            let mapping = Arc::as_ptr(&program.mapping);
            if !mappings[..mapping_count].contains(&mapping) {
                *mappings.get_mut(mapping_count)? = mapping;
                mapping_count += 1;
                bytes = bytes.checked_add(mapping_bytes(&program.mapping)?)?;
            }
            for curve in program.laws.iter().flatten() {
                let pointer = Arc::as_ptr(curve);
                if !curves[..curve_count].contains(&pointer) {
                    *curves.get_mut(curve_count)? = pointer;
                    curve_count += 1;
                    bytes = bytes.checked_add(curve_bytes(curve).ok()?)?;
                }
            }
        }
        Some(bytes)
    }
}
