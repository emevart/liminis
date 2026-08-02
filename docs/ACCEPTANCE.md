# Приёмочные критерии

Стадия считается сделанной, когда перечисленные тесты существуют и зелены.
Не «когда выглядит правильно».

Критерии в SPEC §13 сформулированы человеческим языком: «баланс сходится 10⁶
тиков; видна конвекция и стратификация». Первое проверяемо, второе — нет.
«Видна» означает, что кто-то посмотрел и остался доволен, и такой критерий
нельзя ни поручить, ни повторить, ни провалить. Здесь те же требования записаны
именами тестов.

Правило, из которого всё следует: **критерий, который нельзя назвать именем
теста, не критерий.** Если требование не сводится к проверке, оно либо
сформулировано слишком расплывчато, либо на самом деле проверяется чем-то
другим — и стоит найти чем.

---

## S0. Среда

### Валидатор отказывается грузить

Шестьдесят один отказ. Каждый — класс ошибки, который иначе проявится как
странная динамика через сто тысяч тиков. Порядок тот же, что в таблице §10
`CONFIG_SCHEMA.md`, и число получено пересчётом обоих перечней, а не сложением
дельт из записей журнала: каждая из ADR-056 … ADR-069 берёт базой одно и то же
число девятнадцать.

Последние пятнадцать имён — три класса правил §10, у которых имени теста не
было: ссылочная целостность, область определения и делимость сетки. Правила эти
не новые и не решения — каждое следует из уже принятого, — новыми у них были
только имена, а правило без имени теста по правилу этого документа не критерий.

**Строк в таблице §10 на две больше, чем имён здесь, и это намеренно.**
`initial_layer_naming_an_unknown_substance_is_rejected` и
`initial_layer_side_outside_the_enumeration_is_rejected` пришли с ADR-077 и стоят
ниже, в «Начальных условиях», рядом с остальными именами про сторону слоя: там
они читаются вместе с тем, что стерегут. §10 после них насчитывает шестьдесят
три строки.

```
reaction_unbalanced_by_element_is_rejected
reaction_unbalanced_by_mass_is_rejected
reaction_unbalanced_by_energy_is_rejected
reaction_enthalpy_disagreeing_with_formation_enthalpies_is_rejected
mass_tolerance_above_the_lightest_untracked_substance_is_rejected
substance_lighter_than_its_declared_composition_is_rejected
composition_key_absent_from_conserved_is_rejected
negative_composition_entry_with_nonzero_molar_mass_is_rejected
channel_name_outside_the_enumeration_is_rejected
energy_from_light_is_rejected
reaction_without_t_vmax_is_rejected
reaction_stoichiometry_must_be_positive_integers
substance_without_molar_mass_is_rejected
substance_without_partial_molar_volume_is_rejected
substance_without_settling_radius_is_rejected
settling_substance_with_zero_partial_molar_volume_is_rejected
courant_violation_is_rejected
outflow_bound_violation_is_rejected
velocity_field_without_u_conv_max_is_rejected
structure_length_below_the_temperature_cell_is_rejected
scale_overflow_is_rejected
scale_underflow_is_rejected
substance_dynamic_range_over_2e14_is_rejected
reaction_with_unrepresentable_concentration_spread_is_rejected
stoichiometry_in_storage_units_fits_i32
energy_scale_incompatible_with_a_reaction_enthalpy_is_rejected
substance_count_over_s_max_is_rejected
reaction_count_over_r_max_is_rejected
every_n_ticks_on_diffusive_field_is_rejected
field_over_n_max_substeps_is_rejected
thermal_diffusivity_below_the_fastest_substance_is_rejected
t_ref_outside_the_declared_temperature_range_is_rejected
enthalpy_carried_by_diffusion_over_five_percent_is_rejected
exchange_face_without_a_reservoir_is_rejected
reservoir_missing_a_substance_is_rejected
reservoir_concentration_above_max_conc_is_rejected
exchange_velocity_above_the_courant_limit_is_rejected
an_unknown_process_id_is_rejected
a_duplicate_process_id_is_rejected
calibration_path_that_leads_nowhere_is_rejected
a_reaction_naming_an_unknown_substance_is_rejected
km_missing_for_a_reaction_input_is_rejected
km_naming_a_substance_outside_the_inputs_is_rejected
a_catalyst_outside_the_declared_forms_is_rejected
a_requires_window_is_rejected_until_the_gate_exists
light_enabled_without_an_irradiance_is_rejected
a_negative_surface_irradiance_is_rejected
a_modulation_fraction_outside_the_unit_interval_is_rejected
a_modulation_period_that_is_not_a_whole_number_of_ticks_is_rejected
a_modulation_period_under_three_ticks_is_rejected
a_lit_scenario_is_refused_until_an_energy_sink_exists
a_duplicate_id_in_any_section_is_refused
a_reservoir_naming_an_unknown_substance_is_rejected
every_domain_rule_refuses_its_own_violation
a_not_a_number_never_passes_a_domain_rule
t_out_outside_the_declared_temperature_range_is_rejected
stir_period_missing_with_stirring_on_is_rejected
a_calibration_window_that_is_not_an_interval_is_rejected
a_grid_not_divisible_by_its_coarsest_lod_is_rejected
a_periodic_face_without_its_partner_is_rejected
a_settling_substance_is_refused_until_g_and_the_medium_density_are_named
```

Над перечнем стоят три имени, которые сами отказами не являются и без которых он
ничего не значит. `the_worked_example_passes_every_check` гоняет сценарий §12
`CONFIG_SCHEMA.md` целиком: без него весь список зелен у валидатора,
отвергающего всё подряд, — и он же краснеет, когда очередная проверка задевает
собственный пример корпуса. `every_refusal_names_the_numbers_it_compared`
проходит по таблице «фикстура → ожидаемые подстроки» и требует от каждого
сообщения трёх вещей: имя виновника (или пары «вещество, реакция» там, где
ADR-039 требует пару), обе стороны нарушенного неравенства числами и указание,
что менять. `the_validator_refuses_before_it_derives` закрепляет порядок стадий:
конфиг с опечаткой в имени вещества **и** переполненным масштабом отвергается
сообщением про опечатку, потому что §10 прямо описывает исход «реакция,
отвергнутая не тем сообщением», а `derive`, вызванный первым, регулярно даёт
именно его.

**Одно имя перечня стоит `#[ignore]`, и причина записана в самом атрибуте.**
`scale_underflow_is_rejected` — правило за этим именем пережило свою причину:
после ADR-039 разрешение от `dx` не зависит вовсе, и осмысленным остаётся окно
`dx` из ADR-002, которое §13 п. 17 велит либо записать решением, либо снять имя —
но не молча. Три имени, стоявшие тут рядом с ним, ушли:
`reaction_without_t_vmax_is_rejected` и `an_unknown_process_id_is_rejected`
атрибут уже не носят, а `requires_naming_an_unknown_field_is_rejected` снято
записью ADR-073 вместе с правилом — непустой `requires` стал ошибкой загрузки,
поэтому разрешать имя больше не во что, и на его месте стоит
`a_requires_window_is_rejected_until_the_gate_exists`. Умолчания `enabled` семи
процессов из девяти по-прежнему не назначены ничем (§13 п. 23); свет и поле
скоростей — те два, у которых умолчание назначено решением (ADR-069, ADR-076).

`courant_violation_is_rejected` и `outflow_bound_violation_is_rejected` — разные проверки. Первая про линейную устойчивость, `max(|u|·dt/dx) ≤ 1`. Вторая про неотрицательность: воксель с расходящимся течением отдаёт сумму по исходящим граням, а не максимум, и условие там строже (SPEC §4.2). Конфиг, проходящий первую и валящий вторую, существует, и он даёт отрицательные количества.

**Третьего отказа не будет ни у адвекции, ни у диффузии, и это решено, а не
забыто.** Обе схемы положительны в точной арифметике: у диффузии условие
положительности и условие устойчивости совпадают при `6α ≤ 1`, у адвекции второе
условие уже стоит рядом с первым. Ломает положительность то, чего нет ни в одном
из этих неравенств, — округление потока каждой грани до целой единицы хранения,
— и это не свойство конфига, а свойство состояния поля (ADR-068). Поэтому
недобор принимается с доказанной границей и проверяется по самому буферу, а
блок отказов не растёт ни на одно имя.

Два входа условия Куранта, пришедшие с ADR-067 и ADR-069, **не складываются**.
`u_conv_max` поля скоростей проверяется условием шага `c`, `w_sed` оседания —
собственным одноосевым `w·dt/dx ≤ 1` шага `f`: при расщеплении Ли — Троттера
(ADR-036) каждому оператору достаточно собственного условия. Смещение за тик
складывается из трёх слагаемых — 0.167 вокселя от адвекции, не более одного от
давления, не более одного от оседания, — и это утверждение о модели, а не
неустойчивость.

`reaction_without_t_vmax_is_rejected` стоит здесь с ADR-048, а пары в §10
`CONFIG_SCHEMA.md` не имел до этой правки. Теперь имеет, но ключа `t_vmax` схема
по-прежнему не объявляет: правка ADR-048 не применена, и это видно из таблицы.

`scale_underflow_is_rejected` — про нижнюю границу разрешения: сценарий
экорежима, запущенный при микрорежимном `dx`, обязан отказаться грузиться, а не
считать воду одной единицей.

Три отказа посередине списка пришли из ADR-039. Два из них —
`reaction_with_unrepresentable_concentration_spread_is_rejected` и
`stoichiometry_in_storage_units_fits_i32` — отличаются от прочих тем, что называют
не вещество, а **пару «вещество, реакция»**; третий,
`substance_dynamic_range_over_2e14_is_rejected`, сравнивает `max_conc` и
`typical_conc` одного вещества и о реакциях не знает. Разброс концентраций внутри
одной реакции ограничен: при `sᵢ · max_conc,ⱼ / typ_conc,ᵢ > 2²⁸·β` (ADR-042) коэффициент
хранения `νᵢ` либо переполняет `i32`, либо оказывается больше всего пула вещества
`i` — и тогда реакция не совершается ни разу, молча. Сообщение об отказе обязано
назвать обе стороны: конфиг, в котором виновата пара, нельзя починить, глядя на
одно имя.

`reaction_with_unrepresentable_concentration_spread_is_rejected` проверяется на
настоящем реестре §2.3, а не на выдуманном. Единственная известная в проекте
несовместность — его собственная: вода и протон в одной реакции при `i32` для
воды. Тест обязан её воспроизводить, иначе он проверяет, что валидатор умеет
отказывать, а не что он отказывает там, где надо.

### Вывод масштабов

Масштабы, экспонента экстента и разрядность не задаются, а выводятся (ADR-039,
ADR-040), поэтому проверять их надо как вычисление, а не как объявление.

```
extent_exponent_is_derived_from_the_scarcest_participant
storage_width_is_derived_not_declared
water_is_stored_in_64_bits_in_the_default_registry
reaction_energy_delta_is_integral_after_load
mass_tolerance_is_derived_not_declared
every_substance_occupies_exactly_one_lane
widening_a_substance_does_not_renumber_the_others
mixed_width_storage_matches_uniform_width_storage
a_single_width_registry_allocates_one_field
load_reports_the_restoration_traffic_per_tick
enthalpy_substeps_are_derived_from_thermal_diffusivity
enthalpy_storage_width_is_derived_not_declared
n_max_refusal_names_the_field_and_the_lod_that_fixes_it
enthalpy_at_default_lod_is_rejected_over_n_max
settling_coefficient_is_derived_from_radius_not_declared
stokes_velocity_uses_radius_not_diameter
```

`extent_exponent_is_derived_from_the_scarcest_participant` — прямая защита от
ошибки, ради исправления которой ADR-039 и написан. `e_r`, взятая от самого
обильного участника вместо самого дефицитного, компилируется, грузится и даёт
мир, в котором химия стоит на месте при внешне исправном ledger. Тест подаёт
реакцию с двумя участниками заведомо разной концентрации и смотрит, от которого
получилось число.

`water_is_stored_in_64_bits_in_the_default_registry` выглядит как тест про воду, а
на деле про вывод: если правило разрядности однажды выродится в список имён, он
продолжит проходить. Поэтому рядом стоит `storage_width_is_derived_not_declared` —
он двигает `max_conc` произвольного вещества и требует, чтобы ширина поменялась
следом.

Последние четыре пришли из ADR-056 и проверяют не масштаб, а адресацию: индекс
вещества разрешается в пару «ширина хранения, полоса», причём полоса нумеруется
внутри своего класса ширины, поэтому `lane == s` неверно нигде.
`every_substance_occupies_exactly_one_lane` требует биекции без дыр и без
повторов; `widening_a_substance_does_not_renumber_the_others` запрещает
перенумерацию веществ по ширине, потому что индекс вещества индексирует `nu_sub[]`,
входной вектор генома (ADR-053), метрики и снапшоты;
`a_single_width_registry_allocates_one_field` про законный случай нуля полос в
одном из двух классов.

Четвёртое, `mixed_width_storage_matches_uniform_width_storage`, — несущее и пока
не написано: оно гоняет ядро реакций на разноширинном реестре и на искусственно
одноширинном и требует совпадения результата, а ядра реакций нет. До него ошибка
адресации внутри ядра не ловится ничем — перепутанная полоса читает чужие
количества молча, а ledger сходится, потому что вещество не потеряно, а прочитано
не то.

Последние семь пришли с ADR-057, ADR-061, ADR-062 и ADR-067, и раздела ни одна
из этих записей не назвала — выбран этот, потому что все семь проверяют не
объявление, а вычисление при загрузке.

`load_reports_the_restoration_traffic_per_tick` — про то, что паритет полос есть
свойство конфига, а не кода (ADR-057): порог между `n = 1` и `n = 2` стоит на
`D = dx²/(6·dt) = 1.67·10⁻⁹ м²/с`, H₂S и CH₄ объявлены в четырёх процентах под
ним, и калибровочный сдвиг диффузии меняет счёт копий молча. Загрузчик обязан
напечатать трафик в байтах на тик рядом с числом подшагов, иначе изменение
никак не видно.

`enthalpy_substeps_are_derived_from_thermal_diffusivity` и
`enthalpy_storage_width_is_derived_not_declared` — из ADR-062. Второй несёт
число: поле приращения энергии реакций уезжает в `i64`, воксель растёт с 226 до
**230 байт**, состояние при 128³ — с 474 до **482 МБ**. SPEC §1.2 печатает 226 и
474; спека заморожена (ADR-032), и расхождение живёт здесь.

`n_max_refusal_names_the_field_and_the_lod_that_fixes_it` и
`enthalpy_at_default_lod_is_rejected_over_n_max` стоят рядом с
`field_over_n_max_substeps_is_rejected` из блока отказов, но проверяют не факт
отказа, а его содержание: сообщение обязано назвать поле, его `n` и
`Δlod = ⌈log₄(n/64)⌉`, а второй — что энтальпия при умолчательном `lod = 0`
даёт 84 подшага и отвергается, а не дорожает молча в шестнадцать раз. ADR-061
предписывал второму висеть `#[ignore]` со ссылкой на `CONFIG_SCHEMA.md` §13
п. 9; ADR-062 закрыл этот пункт той же пачкой, так что запускать его есть чем с
первого дня.

`settling_coefficient_is_derived_from_radius_not_declared` и
`stokes_velocity_uses_radius_not_diameter` — из ADR-067, и второй существует
ради множителя четыре. `k = 2r²/9` через радиус, `k = d²/18` через диаметр;
запись `k = r²/18` под именем `settling_radius` даёт скорость ровно вчетверо
меньше истинной, а множитель четыре тише множителя тысяча и переживает
калибровку скорости осадконакопления так же незаметно.

`reaction_energy_delta_is_integral_after_load` — про то, что энтальпия участвует
в реакции наравне с веществом (ADR-041). Объявленная `enthalpy` вещественна, её
коэффициент округляется при загрузке, и тест требует двух вещей сразу: что
округление произошло один раз и до первого тика, и что валидатор назвал
наведённую относительную ошибку числом. Неокруглённый коэффициент дал бы
округление в рантайме на каждом применении — то есть независимое от вещественного,
то есть расходящийся энергетический ledger.

### Сохранение

```
flux_is_antisymmetric
flux_is_the_two_named_crossings_composed
a_small_gradient_in_a_large_pool_still_carries_flux
advection_undershoot_stays_within_the_derived_bound
transport_undershoot_stays_within_the_derived_bound
the_undershoot_bound_is_attained_at_the_stability_limit
a_voxel_on_a_closed_boundary_stays_within_the_five_face_bound
undershoot_over_a_tick_is_bounded_by_the_substep_count
a_starved_voxel_is_overdrawn_rather_than_clamped
a_negative_pool_caps_the_extent_at_zero
a_negative_pool_caps_the_competition_coefficient_at_zero
a_guild_mean_is_not_read_below_the_biomass_floor
advection_alone_conserves_exactly
diffusion_alone_conserves_exactly
transport_of_a_64_bit_substance_conserves_exactly
a_process_returns_with_state_n_in_the_front_buffer_for_every_lane
mixed_substep_lanes_diffuse_as_if_each_were_alone
the_restoration_copies_the_smaller_parity_group
a_lane_no_process_touched_is_not_left_a_tick_stale
energy_fold_from_fine_to_coarse_conserves_exactly
diffusing_matter_does_not_move_enthalpy
absorbed_light_appears_in_enthalpy
solar_in_is_credited_the_same_integer_the_fold_added_to_enthalpy
the_solar_slice_is_overwritten_not_accumulated
the_fold_writes_only_its_own_coarse_index_in_every_output
a_tick_without_the_fold_leaves_solar_in_untouched
the_solar_reduction_is_exact_at_the_full_declared_enthalpy_range
crediting_more_solar_than_the_counter_holds_is_refused_not_wrapped
load_reports_the_energy_counter_ceiling_in_joules
the_solar_term_does_not_depend_on_the_enthalpy_lod
the_solar_term_scales_with_the_tick
a_dark_scenario_moves_no_enthalpy
the_top_layer_absorbs_nothing_when_the_light_is_off
daily_modulation_preserves_the_period_mean_irradiance_exactly
load_reports_the_ticks_to_the_declared_temperature_ceiling
all_reactions_share_one_competition_coefficient
cell_contributions_are_gathered_not_scattered
cell_energy_ledger_closes_over_apoptosis
reaction_alone_conserves_each_element_exactly
competition_scaling_conserves_each_element_exactly
charge_is_tracked_as_a_conserved_quantity_of_zero_mass
pressure_relaxation_conserves_exactly
pressure_relaxation_reaches_hydrostatic_equilibrium
pressure_signal_crosses_the_domain_at_one_voxel_per_tick
sedimentation_conserves_exactly
settling_out_of_the_top_face_appears_in_boundary_exchange
boundary_outflow_appears_in_channel_counter
exchange_face_carries_enthalpy_into_the_energy_counter
the_ghost_cell_is_the_same_value_in_both_buffers
a_domain_reduction_skips_the_ghost_element
an_exchange_face_returns_the_ghost_and_the_ghost_is_its_own_neighbour
an_exchange_axis_paired_with_periodic_is_refused
the_undershoot_bound_counts_the_exchange_face_as_open
ledger_residual_is_zero_over_1e6_ticks
energy_ledger_residual_is_zero_over_1e6_ticks
channel_counters_do_not_overflow_at_1e7_ticks
a_closed_domain_leaves_every_channel_counter_at_zero
domain_sums_do_not_overflow_for_water_at_256_cubed
```

**`flux_is_antisymmetric` — самый важный тест во всём списке**, и одновременно
самый дешёвый. Свойство: `flux(a, b) == −flux(b, a)` при любых `a`, `b`;
проверяется `proptest` на случайных парах. На нём держится сохранение в
gather-форме (ADR-034), и ничто другое его не ловит: функция потока, написанная
через `floor` вместо округления половин от нуля, компилируется, проходит
grep-проверку хука и выглядит правильно.

**Восемь имён недобора пришли из ADR-068, и вместе они заменяют одно обещание,
которое было ложным.** Прежнее `advection_never_produces_negative_amount`
обещало неотрицательность и обещать её не могло: положительны обе схемы только в
точной арифметике, а поток каждой грани округляется до целой единицы хранения, и
шесть округлений по половине дают до трёх лишних единиц у любой семиточечной
flux-form схемы. Ограничитель ван Леера этого не меняет — TVD есть свойство
вещественной арифметики. Поэтому имя переименовано в
`advection_undershoot_stays_within_the_derived_bound`, а граница записана
формулой: `new ≥ (минимум по стенсилю) − ⌊f/2⌋ − 3·⌈разброс/2²⁴⌉`, где `f` —
число открытых граней. Абсолютного числа тут назвать нельзя ни одного: три
единицы ниже `2²³`, шесть ниже `2²⁴`, пятьдесят одна на потолке ADR-039, а у
воды `91 198` единиц — что есть `1.8·10⁻⁷` её пула. Имя про закрытую границу
названо через пять граней, а не через «два», ровно поэтому.
`a_starved_voxel_is_overdrawn_rather_than_clamped` уже существует в коде и до
сих пор не был назван ни в одном документе.

Три имени про деление — `a_negative_pool_caps_the_extent_at_zero`,
`a_negative_pool_caps_the_competition_coefficient_at_zero`,
`a_guild_mean_is_not_read_below_the_biomass_floor` — про то, что ломает
отрицательное количество: не разрядность, а три деления. `ξ_max = minᵢ⌊amountᵢ/νᵢ⌋`
при `amount = −3` даёт отрицательный экстент, и реакция идёт назад, сохраняя
вещество, — оба ledger'а сходятся, и не замечает никто. Первое из трёх —
расхождение с замороженной спекой: формула SPEC §5 ломается не транспортом, а
собой.

Четыре имени ADR-057 — про границу процесса, а не про подшаг. Подшаги перестают
трогать указатели поля, а процесс, продвинувший полосы разным числом подшагов,
восстанавливает инвариант сам: копируется **меньшая по числу полос** группа
паритета, не нечётные. Разница считается: копия нечётных стоила бы 100.7 МБ при
128³ против 25.2, потому что самая широкая полоса реестра — вода — стоит именно
на нечётной стороне.

`flux_is_the_two_named_crossings_composed` и
`a_small_gradient_in_a_large_pool_still_carries_flux` — из ADR-060, и второй
несущий. Порядок операций внутри композиции проверить больше нечем: обе формы
антисимметричны, обе сохраняют точно, и `diffusion_matches_analytic_gaussian_spread`
их не различает — разрыв `1.65·10⁻⁹` лежит под любым разумным допуском. Между
тем «сначала концентрации» у воды при `5.1·10¹¹` единиц теряет всё, что меньше
`ulp`, и бугор в 100 000 единиц перестаёт диффундировать совсем: одна ячейка
против двадцати пяти за четыре тика.

`channel_counters_do_not_overflow_at_1e7_ticks` не гоняет десять миллионов
тиков. Он проверяет тип счётчика и подставляет граничное значение. **Ни одна из
двух ветвей при этом не доказана, и прежняя формулировка — «`i32`
переполняется внутри заявленного горизонта, `i64` нет» — по обеим неверна.** По
энергии: при `k_E = 67` потолок счётчика есть `2⁶³/2⁶⁷ = 62.5 мДж`, а один
освещённый тик 128³ при полном солнце даёт `0.16384 Дж` — 2.62 потолка **за
тик** (ADR-075, ADR-076). По веществу: `i64`-счётчик терпит `9.22·10¹¹` единиц
за тик, то есть при 128³ около `5.63·10⁷` на воксель грани обмена, `0.011 %`
его собственного пула воды, — постоянный градиент такого порядка исчерпывает
счётчик внутри объявленного горизонта. Корпусное «10⁶ тиков по тысяче единиц
дают 10⁹» из `QUANTITIES.md` §3 — заглушка, а не худший случай. Разрядность
счётчиков каналов — открытый вопрос A-20 (в ADR-075 и ADR-076 он назван A-19:
номер был свободен, когда их писали), и операционная форма, делающая потолок
видимым вместо красного, — строка отчёта загрузки
`load_reports_the_energy_counter_ceiling_in_joules`.

**`a_tick_without_the_fold_leaves_solar_in_untouched` носит `#[ignore]`, и
причина записана в атрибуте.** ADR-075 называл его конфигурацию достижимой —
реестр с выключенными реакциями не идёт ни шагом `h`, ни шагом `i′`, — и той же
записью сказал, что места диспетча шага `i′` не создаёт: `process::credit_solar`
приезжает без вызывающего. Значит `SOLAR_IN` не пополняет ни один реестр, срез
`World::solar_in` не пишет никто, и обе половины теста держатся при любой
реализации — в том числе при редукции, унесённой в фазу 5, то есть при ровно том
варианте, который тест заведён отличать. Имя возвращается в строй в тот день,
когда шаг `i′` попадает в диспетч; ослабить его формулировку значило бы оставить
на этом месте видимость живого сторожа.

Последние два пришли из ADR-059 и, по его же словам, самые дешёвые и самые злые.
`a_closed_domain_leaves_every_channel_counter_at_zero` гоняет настоящий транспорт
над тором и требует двух вещей сразу, а не одной: обе невязки — точные нули, и
все шесть каналов не тронуты ни по одному веществу. Первое сохранилось бы и у
процесса, зачисляющего поток через **внутреннюю** грань в `BOUNDARY_EXCHANGE` —
кредиты по домену взаимно сократились бы, — второе нет.

`domain_sums_do_not_overflow_for_water_at_256_cubed` называет и вещество, и сетку
намеренно. Взять худшим случаем 32-битное вещество — именно так возьмёт первый,
кто станет считать, — значит взять не тот: при потолке `2³¹/8` на воксель его
доменная сумма при 128³ есть `5.63·10¹⁴`, что переполняет `i32` ровно в `2¹⁸` раз
и оттого выглядит как довод в пользу `i64`, но остаётся в 16 383 раза ниже
`i64::MAX`. На таком тесте разрядность накопителя не проверяется вовсе: он зелен
и при `i64`, и при `i128`. Худший случай — вода, единственное 64-битное вещество
реестра (ADR-040): `5.12·10¹¹` единиц на воксель, при 256³ — цели S5 из SPEC §13
— доменная сумма `8.59·10¹⁸`, то есть **93% `i64`**. Запас исчезает не в
теоретическом пределе, а на числе, уже записанном в дорожной карте, поэтому
редукция ведётся в `i128`, а тест обязан гоняться именно на воде и именно при
256³. Левая сторона при этом не исчерпывается `amount[]`: гильдийные поля и
`struct_mass` таблицы клеток — тоже моли `BIOMASS` (ADR-059), и их вклад тест
подаёт отдельно.

`sedimentation_conserves_exactly` — имя не новое, но с ADR-067 у него появилось
содержание: один оператор, одна ось, точная антисимметрия потока через грань.
Второго имени для той же проверки запись не заводит, и `settling_alone_…` был бы
дублем, а не соседом: процессом-одиночкой тест делает конфиг, а не имя.
Рядом стоит `settling_out_of_the_top_face_appears_in_boundary_exchange`:
всплывающее вещество (`ρ̄ᵢ < ρ_среды`, та же формула с обратным знаком) покидает
домен через `z_max` и обязано быть дебетовано каналом против ghost-ячейки
резервуара.

Пять имён вокруг `boundary_outflow_appears_in_channel_counter` — про механизм
ghost-ячейки ADR-059, и каждое ловит ошибку, которую **ни одна невязка не видит**.
`the_ghost_cell_is_the_same_value_in_both_buffers`: подшаги чередуют направление
(ADR-057), поэтому ghost, засеянный в один буфер, делает крышку бесконечным
стоком через подшаг — ноль есть законное количество, поток остаётся
антисимметричным, счётчик записывает ровно то, что утекло, и обе невязки сходятся
точно. `a_domain_reduction_skips_the_ghost_element`: резервуар постоянен и
сокращается в разности `after − before`, поэтому врут только абсолютные суммы, а
вместе с ними экспорт объёма и полоса floor/peak worldgen.
`an_exchange_face_returns_the_ghost_and_the_ghost_is_its_own_neighbour`: без
раннего возврата `Grid::coords` декодирует `n_voxels` как координату за концом Z,
и в release ответ зависит от экстентов сетки.
`an_exchange_axis_paired_with_periodic_is_refused` — тот же довод, что у
полупериодической оси, на шаг дальше: у периодической половины сосед за гранью
есть, у обменной он ghost. `the_undershoot_bound_counts_the_exchange_face_as_open`:
у вокселя верхнего слоя при `z_max = exchange` шесть **открытых** граней, а не
пять, и граница ADR-068 для него на единицу шире; имя
`a_voxel_on_a_closed_boundary_stays_within_the_five_face_bound` этого не видит,
оно про закрытую крышку.

`charge_is_tracked_as_a_conserved_quantity_of_zero_mass` пришпиливает тестом
обещание ADR-025, которое до ADR-064 было прозой: `charge = 0.0` в `conserved` и
знаковый `charge = -2` в `composition`. Раздела ADR-064 для него не назвал;
выбран этот, потому что заряд — сохраняемая величина, и проверяется он тем же
элементным балансом.

Тесты «alone» гоняют один процесс с выключенными остальными — это то, ради чего
ADR-018 требует, чтобы процессы включались и выключались по отдельности. С
ADR-065 их конфиги дорожают: перечень процессов замкнут и материализуется
целиком, поэтому выключить остальные можно только выписав `enabled = false` на
каждый — три строки на процесс, двадцать четыре на конфиг.

### Численная верификация

Метод изготовленных решений: берётся гладкая функция, подставляется в уравнение,
невязка добавляется источником, получается точное решение для сравнения.

```
mms_diffusion_converges_at_order_2
mms_advection_converges_at_order_2_on_monotone_data
flux_limiter_falls_back_to_first_order_at_zero_gradient
diffusion_matches_analytic_gaussian_spread
advection_of_a_step_produces_no_new_extrema
light_attenuation_matches_beer_lambert
temperature_from_enthalpy_round_trips
exchange_inflow_falls_back_to_first_order
abiotic_reaction_proceeds_with_empty_catalyst
abiotic_rate_is_independent_of_every_catalyst_field
catalyzed_rate_equals_abiotic_rate_times_catalyst_concentration
```

`temperature_from_enthalpy_round_trips` перестал быть неисполнимым: ADR-062 дал
ему и масштаб, и сетку. Гонять его надо на **грубой** сетке поля энтальпии —
температура величина грубой ячейки, шестьдесят четыре мелких вокселя делят одно
`T`, и `q10` реакции читает температуру накрывающей ячейки, — иначе непонятно,
какой состав он замыкает.

`exchange_inflow_falls_back_to_first_order` — про то, что откат ограничителя на
границе получается даром: при втекании донором служит ghost-ячейка, ячейкой выше
по потоку от неё оказывается она же, числитель отношения `r` обращается в ноль и
`φ(0) = 0`. Цена названа честно: на плоскости втекания численная диффузия
возвращается к `1.25·10⁻⁹ м²/с` против молекулярной кислородной `2.1·10⁻⁹` — но
на 0.78% домена и в один слой.

Три имени ADR-063 — про условную единицу `vmax`, и раздела запись не назвала;
выбран этот, потому что все три сверяют скорость с аналитическим отношением, а
не проверяют сохранение. Первое ловит исход «катализатор нулевой концентрации»:
абиотическая реакция с законным `vmax` обязана давать `ξ > 0`, иначе вся химия
S0 стоит, обе половины инварианта сходятся на `0 = 0` и падать нечему. Второе
ловит фантом: изменение любого поля катализа не меняет `ξ` абиотической реакции.
Третье закрепляет развилку с обеих сторон — при одинаковом числе `vmax`
каталитическая скорость больше абиотической ровно во столько раз, какова
концентрация катализатора.

Порядок сходимости проверяется на паре сеток и обязан выходить на ожидаемый, а
не просто «уменьшаться». Ошибка расщепления первого порядка (ADR-036) видна
здесь как порядок ниже ожидаемого — и это единственное место, где её вообще
видно.

Адвекция ждёт второго порядка, потому что по ADR-054 принят ограничитель ван
Леера. Оговорка в имени теста несущая: изготовленное решение обязано быть
**монотонным вдоль оси прогонки**. TVD-ограничители деградируют до первого
порядка на экстремумах, и на решении с максимумом порядок выйдет между единицей
и двойкой — то есть тест начнёт мерить положение экстремума, а не схему.

`flux_limiter_falls_back_to_first_order_at_zero_gradient` — про частный случай,
который иначе обнаружится делением на ноль в проде: при нулевом градиенте
отношение `r` не определено, и **обе стороны грани** обязаны откатиться
одинаково. Разный откат ломает антисимметрию, и ловит это уже
`flux_is_antisymmetric`, но отдельный тест называет причину.

### Согласованность масштаба

```
integral_quantities_agree_between_dx_and_dx_half
config_schema_declares_no_voxel_dependent_units
```

Первый — тест из SPEC §10, но **не в той форме, в какой §10 его описывает**, и
это первое зафиксированное расхождение с замороженной спекой (ADR-054). Там
сказано «расхождение означает, что воксель просочился в формулу»; после принятия
ограничителя ван Леера это слишком сильно. Численная диффузия падает как `dx²`, а
не как `dx`, поэтому расхождение при удвоении разрешения обязано не исчезать, а
**уменьшаться вчетверо**. Правильная форма теста — проверка порядка, а не
равенство с допуском: допуск пришлось бы подбирать, чего §10 сам же и запрещает,
а порядок выводится из схемы.

Смысл при этом сохраняется полностью. Воксель, просочившийся в формулу, даёт
расхождение, которое при измельчении сетки **не** падает как `dx²` — либо не
падает вовсе, либо падает не с тем порядком. Тест ловит ровно это.

Второй проверяет не значения, а схему: после ADR-035 масштабы выводятся, и в
конфиге не должно остаться ни одного числа, у которого воксель в единицах.

### Детерминизм

```
same_seed_and_config_give_byte_identical_state
different_seed_gives_different_state
different_seed_changes_the_rounding_on_identical_state
run_key_is_derived_from_both_halves_of_the_seed
seed_in_the_scenario_file_is_rejected
reaction_id_is_stable_under_reordering_in_toml
reaction_result_is_independent_of_order_in_toml
stochastic_rounding_is_unbiased_over_1e6_draws
config_hash_ignores_the_calibration_section
the_projection_covers_every_simulated_key
the_canonical_form_reloads_to_the_same_hash
an_omitted_process_section_hashes_as_the_full_default_roster
the_canonical_form_names_every_process_in_the_roster
```

Три имени ADR-058 стоят здесь потому, что сид дошёл до генератора: `rand`
получил четвёртый счётчик `run_key`, и через него расходятся по сиду и химия, и
генерация начальных условий. `different_seed_gives_different_state` до этого
провалить было нечем — менять от сида было нечего.
`run_key_is_derived_from_both_halves_of_the_seed` не косметика: порядок
вложения несимметричен намеренно, `mix(lo) ^ mix(hi)` схлопнул бы сиды, у
которых половины переставлены. `seed_in_the_scenario_file_is_rejected` почти
бесплатен — `deny_unknown_fields` уже делает такую строку ошибкой, — и
существует затем, чтобы это было решением, а не совпадением; это же операционная
форма расхождения с SPEC §12.4, которая говорит «seed в конфиге».
`different_seed_gives_different_state` при этом есть утверждение о **конкретной
паре** сидов, а не теорема: `u64 → u32` схлопывается, и внутри галереи из десяти
тысяч сидов вероятность коллизии `1.2%`.

`the_projection_covers_every_simulated_key` и
`the_canonical_form_reloads_to_the_same_hash` — из ADR-066, и первый ловит то,
чего не ловит компилятор: исчерпывающая деструктуризация не отличает `field: x`
от `field: _`. Тест собирает множества путей ключей у `Config` и у проекции и
требует, чтобы разность в точности равнялась константе `NOT_HASHED`. Полон он
настолько, насколько полна его фикстура: пустой массив таблиц печатается как
`substance = []` и не даёт ни одного вложенного пути, поэтому фикстурой обязан
быть сценарий §12 `CONFIG_SCHEMA.md` целиком — не меньше одной записи каждого
массива, включая `[[calibration]]`.

`an_omitted_process_section_hashes_as_the_full_default_roster` и
`the_canonical_form_names_every_process_in_the_roster` — из ADR-065. Первый есть
§11 п. 2, распространённый на секцию, полного состава которой в файле нет:
конфиг без записи про диффузию и конфиг с выписанным умолчанием обязаны дать
побайтово равные хеши. Второй — единственное, чем подкреплено обещание «видно,
что именно получилось»: `config::canonical` публична, но наружу её не выводит
никто, и без флага `--print-canonical` пользователь, забывший запись, не видит
вообще ничего.

`reaction_id_is_stable_under_reordering_in_toml` — прямое следствие ADR-027:
если идентификатор берётся от позиции в файле, перестановка двух реакций меняет
поток случайных чисел и, значит, прогон, при неизменной семантике.

`reaction_result_is_independent_of_order_in_toml` — соседний и не тот же самый.
Первый ловит подмену идентификатора, второй — последовательное применение
реакций внутри вокселя: если наличие пересчитывается после каждой, порядок
начинает решать, кто съел субстрат первым. Ошибка невидима, потому что баланс при
этом продолжает сходиться (ADR-041).

### Начальные условия

```
same_seed_gives_the_same_initial_state
different_seed_gives_a_different_initial_state
worldgen_respects_declared_max_conc
layer_sides_are_anticorrelated_across_the_boundary
the_layer_side_does_not_change_with_the_seed
a_substance_declared_uniform_has_no_layer_step
the_canonical_form_names_the_layer_side_of_every_substance
initial_layer_naming_an_unknown_substance_is_rejected
initial_layer_side_outside_the_enumeration_is_rejected
the_initial_band_keeps_every_substance_strictly_above_zero
```

Процедурная генерация из шума (SPEC §12.4, ADR-021, ADR-058). Первые два имени —
не синонимы двух имён из «Детерминизма» этажом выше, и разделены намеренно.
`same_seed_and_config_give_byte_identical_state` и
`different_seed_gives_different_state` — про **прогон**: совпадают ли два мира
после тиков. Эти два — про нулевой тик, и проверяются раньше, чем существует
хоть один процесс. Расхождение по сиду, возникшее в генерации, и расхождение,
возникшее в стохастическом округлении химии, лечатся в разных файлах, а
провалившееся имя обязано называть файл.

`same_seed_gives_the_same_initial_state` сверяет все буферы мира, а не только
количества, и вдобавок требует, чтобы свёртка сида жила на хосте: мир,
сгенерированный из младшей половины сида, не есть мир, сгенерированный из его
ключа. `different_seed_gives_a_different_initial_state`, как и старший брат, есть
утверждение о **конкретной паре** сидов, а не теорема, и по той же причине:
`u64 → u32` схлопывается, `1.2%` на галерею из десяти тысяч (ADR-058). Он же
требует доли разошедшихся вокселей, а не `assert_ne!`: одного несовпавшего
вокселя хватило бы, а доля обязана сидеть у единицы — и он же ловит вариант,
отвергнутый ADR-058, при котором ключ, свёрнутый в первый счётчик, даёт те же
числа с переставленными вокселями.

`worldgen_respects_declared_max_conc` стоит здесь, а не в «Выводе масштабов»:
проверяется не вывод, а его соблюдение. `max_conc` есть жёсткий потолок прогона
(ADR-041), а не оценка, и мир, стартовавший над ним, есть неверный старт, а не
начальное условие. Проверки на границе тика, обещанной ADR-041, не написано,
поэтому это имя — единственное, что стоит между сценарием и переполнением,
которое проявится совсем в другом файле. Три утверждения после потолка — против
пустого прохода, потому что потолку удовлетворяет и константа, и поле, к нему
подрезанное: поле не константа, ни один воксель не сидит ровно на потолке (плато
на нём есть подпись клампа, а ADR-041 запрещает кламп прямым текстом), и потолок
промахнут не на ширину округления.

Полоса, внутри которой блуждает начальное условие, ключом не является и
ратифицирована выводом: `excursion = min(typical, max − typical)/2`, целочисленным
делением к нулю (ADR-077). Из одного неравенства `2·excursion ≤ min(typical, H)`
следуют обе границы, обе точные — пол `2·(typical − excursion) ≥ typical` и
потолок `2·(typical + excursion) ≤ typical + max`, — и потому им место в тесте:
это вывод записи, а не константа, выбранная автором теста.
`the_initial_band_keeps_every_substance_strictly_above_zero` гоняет пол по
**всему** реестру фикстуры, а не по одному веществу: отвергнутая форма — доля
запаса до потолка — зелена на воде, у которой типичная концентрация есть 99%
предельной, и красна на кислороде, `SO4` и протоне, то есть ровно на
микрокомпонентах, ради которых заведён динамический диапазон ADR-039. Единичная
форма того же правила живёт в `the_band_is_bounded_by_both_headrooms`, вместе с
вырожденным случаем `typical = 0`.

Три имени про сторону слоя проверяют разное, и подменять одно другим нельзя.
`layer_sides_are_anticorrelated_across_the_boundary` — единственное, что ловит
сторону, разрешённую не на том индексе вещества: оксиклин формируется в другом
месте, оба ledger'а сходятся, и все прочие тесты остаются зелёными. Он же
единственный, кто держит **ориентацию**. Всё, что меряется через границу,
относительно — разбиение читается по собственному отклонению H₂S, — поэтому
глобальная инверсия сторон переименовывает группы вместе с полем и не краснеет
нигде, оставляя мир умолчания стратифицированным вверх ногами. Держат
ориентацию пол и крышка домена, две плоскости, которых рельеф не достаёт: там
слоевой член имеет знак, назначенный объявлением, а `|noise| ≤ UNIT` не даёт
бленду перевести вещество через типичное количество.

`a_substance_declared_uniform_has_no_layer_step` меряет и **ступень** через
границу, и **амплитуду**, и одной ступени мало. Отвергнутая форма
`blended = noise/2` ступени тоже не имеет, а разброс сужает ровно так же, как
снятие слоевого члена, — то есть проходит обе проверки, и ту, что смотрит на
ступень, и ту, что сравнивает ширины. Отличает её отношение двух ветвей на
плоскости пола, где слоевой член присутствует и постоянен: принятая форма даёт
два к одному, отвергнутая — один к одному. Числовых границ заливки в этом тесте
нет и быть не должно: ADR-077 отказывается их печатать, потому что они зависят
от усечения к нулю в `amount_at`, и напечатанное число стало бы благословением
сегодняшнего кода навсегда — а отношение двух прогонов одного генератора числом
фикстуры не является. `the_layer_side_does_not_change_with_the_seed`
отделяет принятую форму от отвергнутого знака слоя по `run_key` — и он
единственный, кто это делает, потому что отвергнутая форма делает
`different_seed_gives_a_different_initial_state` только зеленее.

`the_canonical_form_names_the_layer_side_of_every_substance` — прецедент ADR-065
одной секцией в сторону: умолчание печатается, а не подразумевается, иначе
идентичность прогона несёт утверждение, которого нет ни в одном файле.

**Расхождение с замороженной SPEC §12.4, три места** (ADR-032: спека не
правится). Первое: §12.4 называет три слоя — осадок, вода, воздух, — а
перечисление знает две стороны и «однородно»; воздуха в мире нет ни полем, ни
границей, и слой, которого не считает ни один процесс, был бы значением без
адресата. Второе: §12.4 обещает «всё параметризуемо», а секция параметризует
одну величину и языка генераторов не заводит — шум Уорли, поровая структура и
жерла остаются за её пределами. Третье: §12.4 требует симплекс-шума, а
`worldgen/` реализует решётчатый value noise со smoothstep-затуханием. От шума
здесь требуются **свойства** — `the_lattice_wraps_without_a_seam_in_x_and_y` и
`the_initial_field_has_structure_larger_than_a_voxel`, — а не имя алгоритма;
второе из этих имён гоняется обоими путями, слоевым и однородным, потому что
ADR-077 отверг ключ спектра измерением ровно этой статистики на обоих.

### Наблюдаемое поведение

Здесь «видна конвекция» превращается в число.

```
heated_bottom_produces_net_vertical_transport
density_stratification_persists_without_forcing
oxidation_front_forms_at_predicted_depth
prescribed_velocity_is_divergence_free_bit_for_bit
horizontally_uniform_temperature_produces_no_velocity
substance_with_zero_settling_radius_does_not_move_vertically
```

Это не проверка красоты, а проверка того, что механизм вообще включился.
Порог берётся грубым: конвекция либо переносит вещество вверх на порядок
сильнее диффузии, либо не работает. Точные значения — предмет калибровки, а не
приёмки.

Два первых имени с приходом поля скоростей (ADR-069) переписаны операционно, и
без оговорок они непроверяемы. `heated_bottom_produces_net_vertical_transport`
требует **локализованного** донного источника (SPEC §7, `VENT_BURST`) и `l_c` не
меньше четверти домена: при `l_c = 400 мкм` ячейки замкнуты, усиление идёт как
`Pe^{1/2}` и составляет 1.8× диффузии — порог «на порядок» не берётся не потому,
что механизм не работает. `density_stratification_persists_without_forcing`
ставится с `stir_fraction = 0`, устойчивой стратификацией и **ненулевым**
начальным горизонтальным возмущением `T`. Устойчивая стратификация на нулевом
тике берётся из умолчания `[initial.layer] = "sediment"` (ADR-077): каждое
вещество обогащено в осадке, и это единственный источник вертикального градиента
в корпусе — сценарий, объявивший все вещества `uniform`, лишает это имя
начального условия. Провалиться он может: возмущение
рассасывается кондукцией за `H²/α = 1170 с`, а опрокинуть столб течение успевает
за 768 с. Прежняя форма — «`stir_fraction = 0` и горизонтально однородная `T`» —
была нефальсифицируема: её прошло бы ядро, возвращающее нули.

`prescribed_velocity_is_divergence_free_bit_for_bit` обязан гоняться в обоих
режимах `Q`. Свойство держится не приближённо: хранится потенциал, `u` берётся
узкими разностями хранимых значений, и в дискретном `∇·(∇×A)` каждая компонента
входит дважды с противоположными знаками — сокращение происходит на одном и том
же округлённом числе. Глобальный множитель поверх `u` этим свойством не обладал
бы, и ровно поэтому рантайм-нормировки в решении нет.

`substance_with_zero_settling_radius_does_not_move_vertically` — из ADR-067:
`r = 0` не магическое значение, `k = 2·0²/9` есть точный ноль, и хост просто не
заводит процесс для вещества с нулевой `w`.

### Наблюдение

Соседний заголовок называется почти так же и стоит про другое. «Наблюдаемое
поведение» — про физику, которую мир обязан показать. Здесь — про аппарат,
которым это записывается: поток метрик ADR-037. Аппарат нуждается в собственной
приёмке ровно потому, что его отказы не роняют ни одного теста выше: мир
продолжает считаться правильно, а прогон перестаёт быть интерпретируемым.

```
the_first_record_is_the_run_header
the_residual_is_written_even_when_it_is_zero
a_truncated_tail_does_not_lose_the_rest
```

`the_first_record_is_the_run_header` — две половины, и вторая может проваливаться
годами. Первая: заголовок существует **до** первого тика. Писатель, выдающий его
лениво на первом тике, оставляет файл без заголовка ровно у того прогона, который
умер до тика один, — то есть у того, который и будут читать. Вторая: перечень
колонок не врёт. Заголовок, перечисляющий колонки, которых нет среди ключей
тиковой записи, — валидный NDJSON, лгущий о себе, и больше в корпусе этого не
видит никто; поэтому множества ключей сравниваются в обе стороны.

`the_residual_is_written_even_when_it_is_zero` — несущее требование ADR-037:
записанный ноль есть доказательство, что проверка выполнялась, а отсутствие
строки неотличимо от отключённой проверки. Тесту скармливается замкнутый домен
S0, где обе невязки точно нулевые, потому что это ровно тот тик, под которым
всякая удаляющая колонку оптимизация выглядит безобидно: `if r != 0`, список
пропуска нулей, «компактный» режим — каждая оставляет валидный JSON и не роняет
ничего, так как `assert_closed` живёт в другом месте и только в debug. Пройти
тест печатью константы нельзя: второй тик несёт ненулевые невязки. Колонок
невязки на одну больше числа веществ — ADR-028 делает инвариант двойным, и по
энергии он не выводится из вещественного.

`a_truncated_tail_does_not_lose_the_rest` — первое из трёх свойств, ради которых
ADR-037 выбрал NDJSON. Свойство относится к файлу целиком, поэтому проверяется на
каждом байтовом смещении, где прогон может умереть, и на тех входах, которые
единственно способны его сломать: `substance.id` — свободная строка TOML,
`Registry::new` стережёт дубликаты и число тридцать один и ничего про символы,
так что идентификатор с переводом строки достижим из сценария, а не гипотетичен.
Неэкранированный, он разрезает одну запись на две, и обе выглядят как NDJSON для
построчного читателя — с этого места свойство не держится, и не говорит об этом
никто. Оборванный хвост при этом сообщается отдельно, а не выдаётся как запись:
читатель, вернувший его записью, разобрал бы усечённый объект, и попавшие в него
числа выглядели бы полными.

`WORLD_FORMAT_VERSION` этими тремя не двигается и двигаться не должен (ADR-020):
поток метрик не меняет динамику, а перечень колонок версионируется отдельно —
удаление колонки обесценивает все прошлые прогоны, добавление нет. CI проверяет
только что версия сдвинулась, когда тронут стерегомый путь, и никогда — что она
устояла, когда не тронут ни один.

Критериев второго потока — снапшота для рестарта — здесь пока нет, и это не
упущение. Запись и чтение существуют и покрыты юнит-тестами, но три вещи, от
которых зависит формат, не решены и решаются не тестом: режим `Q` в файле, момент
сдвига версии формата и вынос записи из основного потока. Они помечены
`TODO(snapshot-q)`, `TODO(snapshot-layout)` и `TODO(snapshot-offthread)` в
`observe/snapshot.rs`; имена приёмочных тестов встанут сюда, когда решения
появятся в журнале.

### Вьюер

Третий читатель мира после потока метрик и снапшота, и единственный, у которого
читатель — человек. Отсюда особая форма его отказов: неверная картинка выглядит
правдоподобно. Смещённый на байт заголовок объёма рисует облако, транспонированный
объём рисует облако, объём с write-стороны буфера рисует облако, а нули нужной
длины рисуют мёртвый мир — и ни одно из четырёх не роняет ничего выше по стеку,
потому что количества при этом не менялись и ни одна проверка сохранения этого не
видит. Транспорт — четыре HTTP-маршрута (ADR-070), квантование — против
объявленного `max_conc` (ADR-072), невязка — трёхзначная (ADR-071).

```
the_server_serves_a_volume_of_the_declared_shape
a_volume_byte_is_fixed_by_the_declared_maximum_not_by_the_frame
the_volume_is_indexed_the_way_the_grid_is
the_volume_reads_the_front_buffer_and_never_the_write_side
an_unknown_field_is_a_404_and_not_an_empty_volume
the_profile_runs_from_the_floor_upwards
the_state_route_reports_both_residuals
the_residual_is_unreported_before_the_first_tick
the_published_residual_is_computed_and_not_written
the_reported_matter_residual_does_not_cancel_between_substances
a_dead_simulation_stops_claiming_a_closed_ledger
control_pause_actually_stops_the_tick_counter
step_advances_exactly_one_tick_while_paused
the_reported_rate_is_measured_and_not_the_requested_one
an_unknown_control_action_is_refused_rather_than_ignored
a_seed_past_two_to_the_53_is_not_truncated_by_the_page
a_substance_id_that_would_break_the_json_is_escaped
the_four_routes_are_the_ones_the_viewer_asks_for
the_viewer_is_served_whole
an_unknown_route_is_a_404_and_not_the_viewer
the_wrong_method_is_a_405_that_says_which_one_was_wanted
```

`the_server_serves_a_volume_of_the_declared_shape` проверяет заголовок побайтно
именно потому, что страница его не проверяет вовсе: она читает `DataView(buf, 0,
24)` и `Uint8Array(buf, 24)` и разъехавшуюся границу не заметит. В том же тесте —
`scale`: число, от которого не зависит ни один пиксель (шейдер берёт нормированную
выборку), а значит ошибка в нём невидима глазами и видна только тому, кто читает
объём программой.

`a_volume_byte_is_fixed_by_the_declared_maximum_not_by_the_frame` — единственный,
кто ловит нормировку на максимум кадра, и он же объясняет, почему её надо ловить:
байт одного и того же вокселя обязан не измениться, когда изменился только
экстремум поля. Количества при этом те же, поэтому ни один тест на сохранение
здесь не срабатывает.

`the_volume_reads_the_front_buffer_and_never_the_write_side` ловит два промаха
разом — `lane == s` (ADR-056) и чтение write-стороны (ADR-057), — и оба
недоказуемы на произвольной фикстуре. Поэтому тест сначала утверждает, что
фикстура **способна** их различить: что хотя бы одно вещество лежит на дорожке
другого номера и что хотя бы у одной дорожки две стороны буфера различны. Без этих
двух утверждений он зелен на мире, где промах ненаблюдаем.

`the_state_route_reports_both_residuals` и два соседних имени делят между собой
трёхзначность ADR-071: число на завершённом тике, `null` до первого тика,
`null` после смерти потока симуляции. Слить два последних состояния в ноль — ровно
то, что ADR-037 называет неотличимым от отключённой проверки, только под зелёной
плашкой.

`the_published_residual_is_computed_and_not_written` — тот же несущий вопрос, но
про сам маршрут, и здесь у него нет естественного входа. Все процессы S0 сохраняют
каждое вещество точно, а `Ledger::assert_closed` срабатывает **внутри**
`Tick::advance` в debug-сборке, — значит ни один тик, доступный тесту, не может
дойти до публикации с дырой, и константа `matter: 0` осталась бы зелёной во всех
маршрутных тестах выше. Поэтому дыра делается тестом прямо в аккумуляторах, из
которых публикация читает, и делается несимметричной: `+7` у одного вещества и
`−3` у другого дают `+7` как написано и `−7` при переставленных местами
аккумуляторах. Одна строка порядка при этом не покрыта и покрыта быть не может —
`advance_one` говорит, какая именно и чем это кончится.

`control_pause_actually_stops_the_tick_counter` состоит из двух половин, и первая
существует ради второй: без утверждения, что счётчик **двигался**, тест зелен на
симуляторе, который не запускался вовсе. Пауза проверяется не флагом, а двумя
измерениями счётчика через промежуток: флаг, который читает только сам себя,
ставится и в цикле, который продолжает считать.

`a_seed_past_two_to_the_53_is_not_truncated_by_the_page` и
`a_substance_id_that_would_break_the_json_is_escaped` — два способа для страницы
показать чужой прогон и не сказать об этом. `JSON.parse` режет за 2^53, поэтому
`seed` уезжает строкой; `substance.id` — свободная строка TOML, и кавычка в ней
разрывает объект, страница падает в свой `catch` и печатает «Lost the simulator»,
то есть винит связь.

Живут эти тесты в `crates/liminis/src/{serve,http,main}.rs` — в приватных `mod
tests` внутри бинарного крейта, а не в `tests/`, потому что маршруты и роутер не
экспортируются: снаружи бинаря нет ни `Sim`, ни `Response`. Это и есть причина,
по которой перечень имён нужен здесь: иначе критерий не находится ничем, кроме
чтения исходника.

Одного критерия в этом списке нет: **ни один сценарий в `configs/` не строит мир**
(ADR-070 называет эту цену прямо). Мир, на котором гоняются все имена выше, —
`#[cfg(test)]`-фикстура внутри `serve.rs`, и копировать её в `configs/` нельзя:
`c_p`, `enthalpy_formation` и `partial_molar_volume` в ней помечены как заглушки,
потому что этих величин не объявляет в корпусе никто (`CONFIG_SCHEMA.md` §13,
пункт 23). Имя `every_shipped_scenario_can_be_served` встанет сюда вместе с
решением, откуда берутся термохимические величины веществ, — вопрос A-18.

### Производительность

```
ticks_per_second_above_threshold_on_reference_scenario
```

Порог в CI (E-3). Производительность деградирует по пять процентов за PR
незаметно, пока не станет в пять раз медленнее.

---

## Что где живёт

Эталоны для золотых тестов — `tests/golden/`. Каталог защищён хуком
`golden-guard`: перезаписать эталон можно только осознанно, через
`LIMINIS_BLESS_GOLDEN=1`. Инструмент, способный переписать эталон ради зелёного
теста, эталоном не является.

Тесты валидатора и свойств — обычные юнит- и `proptest`-тесты рядом с кодом.
Длинные прогоны на 10⁶ тиков — отдельная цель, не в основном `cargo test`:
в CI гоняется укороченная версия, полная — по расписанию.

---

## Дальнейшие стадии

Критерии для S1′ и далее пишутся, когда стадия становится следующей. Писать их
сейчас — то же самое проектирование вперёд, от которого предостерегает
`NORTH_STAR.md`, только под видом тестов.

Одно исключение стоит зафиксировать заранее, потому что оно влияет на
архитектуру, а не на приёмку: **операциональные критерии перехода** (B-7). Их
надо записать до первых эволюционных прогонов, иначе post-hoc неизбежен —
найдётся красивая структура, и её объявят тем самым переходом.
