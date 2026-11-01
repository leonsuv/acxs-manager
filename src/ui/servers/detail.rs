//! The panel on the right of the Servers page: everything needed to join the selected server
//! without scrolling. The Join button sits at the top and always names the car it will use.
//! Below it, a searchable grid of the server's cars (the ones you own and that have a free slot
//! first), the skins of the chosen car, and the details folded away.
//!
//! Double-click a car to join with it at once.

use super::*;

/// What the panel needs to know, read-only.
pub(super) struct DetailIn<'a> {
    pub entry: &'a Entry,
    pub live: Live,
    pub lobby: Option<&'a LobbyServer>,
    /// The server is on the watch list (so it can be removed rather than added).
    pub in_list: bool,
    pub tracks: &'a [TrackEntry],
    pub cars: &'a [CarEntry],
}

/// What the user can change in the panel. Lives in [`Servers`] so it survives between frames.
pub(super) struct Edit<'a> {
    pub pick_model: &'a mut Option<String>,
    pub pick_skin: &'a mut Option<String>,
    pub driver: &'a mut String,
    pub password: &'a mut String,
    pub car_query: &'a mut String,
    pub installed_only: &'a mut bool,
}

/// Something the user asked for.
pub(super) enum Action {
    Join(JoinRequest),
    CheckNow,
    CopyAddress,
    Remove,
    AddToList,
    ToggleFavorite,
}

type Group = (String, usize, usize, Vec<(String, usize)>);

/// The panel's contents. Returns what the user asked for this frame.
pub(super) fn show(ui: &mut egui::Ui, input: &DetailIn, edit: &mut Edit) -> Vec<Action> {
    let mut actions = Vec::new();
    let live = &input.live;
    let status = live.status.as_ref();
    let (track, track_id, track_layout) = match status.and_then(|s| s.track.as_deref()) {
        Some(raw) => resolve_track(raw, input.tracks),
        None => (None, String::new(), None),
    };
    let groups: Vec<Group> = status.map(|s| group_cars(&s.cars)).unwrap_or_default();
    let local = |model: &str| input.cars.iter().find(|c| c.id == model);

    // A sensible car is always chosen, so Join works with one click: the remembered one if it is
    // still on the server, else the first car you own that has a free slot.
    let valid = edit.pick_model.as_deref().is_some_and(|m| groups.iter().any(|g| g.0 == m));
    if !valid {
        *edit.pick_skin = None;
        *edit.pick_model = groups
            .iter()
            .find(|g| g.2 > 0 && local(&g.0).is_some())
            .or_else(|| groups.iter().find(|g| local(&g.0).is_some()))
            .map(|g| g.0.clone());
    }

    banner(ui, input, status, track, &track_layout);
    ui.add_space(8.0);

    // Facts, one line.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        match status {
            Some(s) => {
                widgets::pill(ui, &format!("{}/{} players", s.clients.unwrap_or(0), s.max_clients.unwrap_or(0)), OK);
                if let Some(code) = &s.session {
                    widgets::pill(ui, session_name(code), INFO);
                }
                match (track, s.track.is_some()) {
                    (Some(t), _) => {
                        widgets::pill(ui, &t.name, MUTED);
                        if let Some(l) = &track_layout {
                            widgets::pill(ui, l.strip_prefix("layout_").unwrap_or(l), MUTED);
                        }
                    }
                    (None, true) => widgets::pill(ui, &format!("{track_id} (not installed)"), WARN),
                    _ => {}
                }
            }
            None => {
                let text = if live.checking { "Checking…" } else { "Offline" };
                widgets::pill(ui, text, FAINT);
            }
        }
        if input.lobby.is_some_and(|l| l.csp_required) {
            widgets::pill(ui, "needs CSP", INFO);
        }
        if let Some(ms) = live.latency_ms {
            widgets::pill(ui, &format!("{ms} ms"), MUTED);
        }
        if !input.entry.host.is_empty() {
            ui.label(RichText::new(input.entry.key()).size(12.0).color(FAINT));
        }
    });
    if let Some(error) = &live.error {
        ui.label(RichText::new(error).size(12.5).color(DANGER));
    }
    ui.add_space(8.0);

    // The action bar.
    let chosen = edit.pick_model.clone();
    let chosen_car = chosen.as_deref().and_then(local);
    let needs_password = input.lobby.is_some_and(|l| l.password);
    let mut problems: Vec<String> = Vec::new();
    if status.is_none() {
        problems.push("The server does not answer right now.".into());
    }
    if status.is_some_and(|s| s.track.is_some()) && track.is_none() {
        problems.push(format!("You do not have the track {track_id}."));
    }
    if chosen.is_some() && chosen_car.is_none() {
        problems.push("You do not have this car. Pick one marked as installed.".into());
    }
    if status.is_some() && chosen.is_none() {
        problems.push("Pick a car below.".into());
    }
    if needs_password && edit.password.trim().is_empty() {
        problems.push("This server needs a password.".into());
    }
    let can_join = problems.is_empty();

    theme::card_flat().show(ui, |ui| {
        ui.set_width(ui.available_width());
        let label = match (chosen_car, edit.pick_skin.as_deref()) {
            (Some(car), Some(skin)) => format!("JOIN  ·  {}  ·  {skin}", car.name),
            (Some(car), None) => format!("JOIN  ·  {}", car.name),
            _ => "JOIN".to_string(),
        };
        let button = widgets::primary_button(&label).min_size(vec2(ui.available_width(), 46.0));
        if ui.add_enabled(can_join, button).clicked() {
            actions.push(Action::Join(JoinRequest {
                entry: input.entry.clone(),
                model: edit.pick_model.clone(),
                skin: edit.pick_skin.clone(),
            }));
        }
        for problem in &problems {
            ui.label(RichText::new(problem).size(12.5).color(WARN));
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Race as").color(MUTED));
            ui.add(egui::TextEdit::singleline(edit.driver).desired_width(120.0).margin(egui::Margin::symmetric(8, 5)));
            if needs_password {
                ui.label(RichText::new("Password").color(MUTED));
                ui.add(
                    egui::TextEdit::singleline(edit.password)
                        .password(true)
                        .desired_width(110.0)
                        .margin(egui::Margin::symmetric(8, 5)),
                );
            }
            ui.menu_button("More", |ui| {
                if ui.button("Copy address").clicked() {
                    actions.push(Action::CopyAddress);
                    ui.close();
                }
                if ui.button("Check now").clicked() {
                    actions.push(Action::CheckNow);
                    ui.close();
                }
                if input.in_list {
                    if ui.button(if input.entry.favorite { "Remove star" } else { "Star" }).clicked() {
                        actions.push(Action::ToggleFavorite);
                        ui.close();
                    }
                    if ui.button(RichText::new("Remove from my servers").color(DANGER)).clicked() {
                        actions.push(Action::Remove);
                        ui.close();
                    }
                } else if ui.button("Add to my servers").clicked() {
                    actions.push(Action::AddToList);
                    ui.close();
                }
            });
        });
    });
    ui.add_space(10.0);

    // Cars.
    if !groups.is_empty() {
        car_picker(ui, input, edit, &groups, &mut actions);
    }

    // Details, folded away.
    if let Some(s) = status {
        ui.add_space(8.0);
        egui::CollapsingHeader::new(RichText::new(format!("Drivers ({})", s.players.len())).font(theme::bold(13.5)))
            .id_salt(("srv_drivers", input.entry.key()))
            .show(ui, |ui| {
                if s.players.is_empty() {
                    ui.label(RichText::new("Nobody connected right now.").color(MUTED));
                }
                for driver in &s.players {
                    ui.label(RichText::new(driver).size(13.5));
                }
            });
        egui::CollapsingHeader::new(RichText::new("Server info").font(theme::bold(13.5)))
            .id_salt(("srv_info", input.entry.key()))
            .show(ui, |ui| raw_json(ui, &s.raw_info));
        egui::CollapsingHeader::new(RichText::new(format!("Entry list ({} slots)", s.cars.len())).font(theme::bold(13.5)))
            .id_salt(("srv_entries", input.entry.key()))
            .show(ui, |ui| raw_json(ui, &s.raw_entries));
    }
    actions
}

/// The server's track as a slim banner with its name over it.
fn banner(ui: &mut egui::Ui, input: &DetailIn, status: Option<&ServerStatus>, track: Option<&TrackEntry>, layout: &Option<String>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 84.0), Sense::hover());
    let round = CornerRadius::same(12);
    let preview = track.and_then(|t| media::track_preview(t, layout.as_deref()));
    match preview.as_deref() {
        Some(path) => media::paint_cover(ui, rect, path, round),
        None => media::paint_image(ui, rect, None, "", if status.is_some() { ACCENT } else { FAINT }, round),
    }
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), Color32::from_black_alpha(40));
    mesh.colored_vertex(rect.right_top(), Color32::from_black_alpha(40));
    mesh.colored_vertex(rect.right_bottom(), Color32::from_black_alpha(220));
    mesh.colored_vertex(rect.left_bottom(), Color32::from_black_alpha(220));
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
    let name = status
        .and_then(|s| s.name.clone())
        .or_else(|| input.lobby.map(|l| l.name.clone()))
        .unwrap_or_else(|| input.entry.key());
    let mut job = egui::text::LayoutJob::simple(name, theme::bold(15.5), Color32::WHITE, rect.width() - 24.0);
    job.wrap.max_rows = 2;
    job.wrap.break_anywhere = false;
    let galley = ui.painter().layout_job(job);
    let pos = egui::pos2(rect.left() + 12.0, rect.bottom() - 10.0 - galley.size().y);
    ui.painter().galley(pos, galley, Color32::WHITE);
    ui.painter().rect_stroke(rect, round, egui::Stroke::new(1.0, theme::BORDER), egui::StrokeKind::Inside);
}

/// The grid of cars, skins of the chosen one, and a search box.
fn car_picker(ui: &mut egui::Ui, input: &DetailIn, edit: &mut Edit, groups: &[Group], actions: &mut Vec<Action>) {
    let local = |model: &str| input.cars.iter().find(|c| c.id == model);
    let free_total: usize = groups.iter().map(|g| g.2).sum();
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("CHOOSE YOUR CAR  ·  {free_total} free")).font(theme::bold(11.0)).color(FAINT));
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(edit.car_query)
                .hint_text("Search cars")
                .desired_width((ui.available_width() - 150.0).max(100.0))
                .margin(egui::Margin::symmetric(8, 5)),
        );
        widgets::toggle(ui, edit.installed_only);
        ui.label(RichText::new("Mine only").size(12.5).color(MUTED));
    });
    ui.add_space(6.0);

    // Cars you own with free slots first, then owned but full, then the rest.
    let query = edit.car_query.trim().to_lowercase();
    let mut shown: Vec<&Group> = groups
        .iter()
        .filter(|g| {
            let name = local(&g.0).map_or(g.0.clone(), |c| c.name.clone());
            (query.is_empty() || name.to_lowercase().contains(&query) || g.0.to_lowercase().contains(&query))
                && (!*edit.installed_only || local(&g.0).is_some())
        })
        .collect();
    shown.sort_by_key(|g| (local(&g.0).is_none(), g.2 == 0, local(&g.0).map_or(g.0.to_lowercase(), |c| c.name.to_lowercase())));

    if shown.is_empty() {
        ui.label(RichText::new("No car matches. Switch off \"Mine only\" to see cars you do not have.").color(MUTED));
    }

    let gap = 8.0;
    let available = ui.available_width();
    let columns = ((available + gap) / (112.0 + gap)).floor().max(2.0);
    let width = ((available - gap * (columns - 1.0)) / columns).floor();
    let height = (width * 9.0 / 16.0).round() + 30.0;
    egui::ScrollArea::vertical()
        .id_salt("srv_cars")
        .max_height(height * 3.0 + gap * 2.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(gap, gap);
            ui.horizontal_wrapped(|ui| {
                for group in shown {
                    let (model, _total, free, skins) = group;
                    let car = local(model);
                    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
                    let selected = edit.pick_model.as_deref() == Some(model.as_str());
                    let hover = ui.ctx().animate_bool_with_time(response.id, response.hovered(), 0.1);
                    ui.painter().rect_filled(rect, CornerRadius::same(9), theme::CARD.lerp_to_gamma(theme::CARD_HOVER, hover));
                    let image_rect = Rect::from_min_size(rect.min, vec2(width, height - 30.0));
                    let first_skin = skins.iter().find(|s| s.1 > 0).or(skins.first()).map(|s| s.0.as_str());
                    let preview = car.and_then(|c| media::car_preview(&c.path, first_skin));
                    media::paint_image(ui, image_rect, preview.as_deref(), model, INFO, media::top_round_by(9));
                    let title = car.map_or(model.as_str(), |c| c.name.as_str());
                    let galley = media::clipped(ui, title, theme::bold(12.0), TEXT, width - 12.0);
                    ui.painter().galley(rect.left_bottom() + vec2(6.0, -23.0), galley, TEXT);
                    let (text, color) = if car.is_none() {
                        ("not installed".to_string(), WARN)
                    } else if *free > 0 {
                        (format!("{free} free"), OK)
                    } else {
                        ("full".to_string(), FAINT)
                    };
                    media::chip(ui, image_rect.left_top() + vec2(5.0, 5.0), Align2::LEFT_TOP, &text, color);
                    if car.is_none() || *free == 0 {
                        ui.painter().rect_filled(image_rect, media::top_round_by(9), Color32::from_black_alpha(110));
                    }
                    let stroke = if selected { egui::Stroke::new(2.0, ACCENT) } else { egui::Stroke::new(1.0, theme::BORDER) };
                    ui.painter().rect_stroke(rect, CornerRadius::same(9), stroke, egui::StrokeKind::Inside);
                    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(title);
                    if response.clicked() || response.double_clicked() {
                        if edit.pick_model.as_deref() != Some(model.as_str()) {
                            *edit.pick_skin = None;
                        }
                        *edit.pick_model = Some(model.clone());
                    }
                    if response.double_clicked() && car.is_some() && *free > 0 {
                        actions.push(Action::Join(JoinRequest {
                            entry: input.entry.clone(),
                            model: Some(model.clone()),
                            skin: None,
                        }));
                    }
                }
            });
        });

    // Skins of the chosen car.
    let Some((model, _, _, skins)) = groups.iter().find(|g| edit.pick_model.as_deref() == Some(g.0.as_str())) else {
        return;
    };
    if skins.len() > 1 {
        let car = local(model);
        ui.add_space(8.0);
        ui.label(RichText::new(format!("SKIN  ·  {} to choose from", skins.len())).font(theme::bold(11.0)).color(FAINT));
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for (skin, free) in skins {
                let (rect, response) = ui.allocate_exact_size(vec2(72.0, 41.0), Sense::click());
                let preview = car.and_then(|c| media::car_preview(&c.path, Some(skin)));
                media::paint_image(ui, rect, preview.as_deref(), skin, INFO, CornerRadius::same(6));
                if *free == 0 {
                    ui.painter().rect_filled(rect, CornerRadius::same(6), Color32::from_black_alpha(170));
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, "taken", FontId::proportional(11.0), FAINT);
                }
                let selected = edit.pick_skin.as_deref() == Some(skin.as_str());
                let stroke = egui::Stroke::new(if selected { 2.0 } else { 1.0 }, if selected { ACCENT } else { theme::BORDER_STRONG });
                ui.painter().rect_stroke(rect, CornerRadius::same(6), stroke, egui::StrokeKind::Inside);
                if response
                    .on_hover_text(format!("{skin}  ·  {free} free"))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    *edit.pick_skin = Some(skin.clone());
                }
            }
        });
    }
}
