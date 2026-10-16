//! `/f` faction command tree — port of `factions.commands.FactionsCommandExecutor`
//! and the player subcommands.
//!
//! The original relied on a WorldEdit-style selection wand for claiming. Because the
//! port has no selection system yet, `/f claim` claims the single chunk the player
//! stands in (the classic "chunk claim" mode). This is documented in `PORTING.md`.

use pumpkin_plugin_api::command::{
    ArgumentType, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType,
};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Player, Server};

use super::{command, double_arg, int_arg, register_op_permission, register_public_permission, reply, string_arg};
use crate::config::Config;
use crate::core;
use crate::factions::{member, Claim, ClaimManager, Faction, FactionsManager, Role};
use crate::glass::GlassType;
use crate::lang::Lang;
use crate::state::{state, AppState, ChunkState};
use crate::timers::{PendingAction, TimerType};
use crate::util;

#[derive(Clone, Copy)]
enum Action {
    Create,
    Disband,
    Invite,
    Join,
    Leave,
    Kick,
    Promote,
    Demote,
    Leader,
    Rename,
    Chat,
    Claim,
    Unclaim,
    UnclaimAll,
    Map,
    Territory,
    TerritoryCoords,
    Help,
    SetHome,
    Home,
    Stuck,
    Open,
    ShowSelf,
    ShowNamed,
    List,
    TopDefault,
    TopMetric,
    Deposit,
    Withdraw,
    Ally,
    Unally,
    AdminSetDtr,
    AdminSetPoints,
    AdminSetBalance,
    AdminSetLives,
    AdminForceKick,
    AdminForceLeader,
    AdminFreeze,
    AdminThaw,
    AdminSave,
    HelpPage,
}

struct FactionExecutor(Action);

impl CommandHandler for FactionExecutor {
    fn handle(&self, sender: CommandSender, server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        if let Action::Help = self.0 {
            help(&sender, 1);
            return Ok(1);
        }
        if let Action::HelpPage = self.0 {
            let page = int_arg(&args, "page").unwrap_or(1) as usize;
            help(&sender, page);
            return Ok(1);
        }
        run(self.0, &sender, &server, &args);
        Ok(1)
    }
}

pub fn register(context: &Context) -> Result<(), String> {
    register_public_permission(context, "lazarus:command.faction", "Use the faction commands")?;
    register_op_permission(context, "lazarus:command.faction.admin", "Use the faction admin commands")?;

    let str_node = |name: &str| CommandNode::argument(name, &ArgumentType::String(StringType::SingleWord));
    let int_node = |name: &str| CommandNode::argument(name, &ArgumentType::Integer((None, None)));

    let root = command(&["f", "faction", "fac", "factions"], "Faction commands")
        .execute(FactionExecutor(Action::Help))
        .then(
            CommandNode::literal("help")
                .execute(FactionExecutor(Action::Help))
                .then(int_node("page").execute(FactionExecutor(Action::HelpPage))),
        )
        .then(CommandNode::literal("create").then(str_node("name").execute(FactionExecutor(Action::Create))))
        .then(CommandNode::literal("new").then(str_node("name").execute(FactionExecutor(Action::Create))))
        .then(CommandNode::literal("disband").execute(FactionExecutor(Action::Disband)))
        .then(CommandNode::literal("invite").then(str_node("player").execute(FactionExecutor(Action::Invite))))
        .then(CommandNode::literal("inv").then(str_node("player").execute(FactionExecutor(Action::Invite))))
        .then(CommandNode::literal("join").then(str_node("name").execute(FactionExecutor(Action::Join))))
        .then(CommandNode::literal("accept").then(str_node("name").execute(FactionExecutor(Action::Join))))
        .then(CommandNode::literal("leave").execute(FactionExecutor(Action::Leave)))
        .then(CommandNode::literal("kick").then(str_node("player").execute(FactionExecutor(Action::Kick))))
        .then(CommandNode::literal("promote").then(str_node("player").execute(FactionExecutor(Action::Promote))))
        .then(CommandNode::literal("demote").then(str_node("player").execute(FactionExecutor(Action::Demote))))
        .then(CommandNode::literal("leader").then(str_node("player").execute(FactionExecutor(Action::Leader))))
        .then(CommandNode::literal("rename").then(str_node("name").execute(FactionExecutor(Action::Rename))))
        .then(CommandNode::literal("chat").execute(FactionExecutor(Action::Chat)))
        .then(CommandNode::literal("c").execute(FactionExecutor(Action::Chat)))
        .then(CommandNode::literal("claim").execute(FactionExecutor(Action::Claim)))
        .then(CommandNode::literal("unclaim").execute(FactionExecutor(Action::Unclaim)))
        .then(CommandNode::literal("unclaimall").execute(FactionExecutor(Action::UnclaimAll)))
        .then(CommandNode::literal("map").execute(FactionExecutor(Action::Map)))
        .then(CommandNode::literal("territory").execute(FactionExecutor(Action::Territory)).then(
            int_node("x").then(int_node("z").execute(FactionExecutor(Action::TerritoryCoords)))
        ))
        .then(CommandNode::literal("sethome").execute(FactionExecutor(Action::SetHome)))
        .then(CommandNode::literal("home").execute(FactionExecutor(Action::Home)))
        .then(CommandNode::literal("stuck").execute(FactionExecutor(Action::Stuck)))
        .then(CommandNode::literal("open").execute(FactionExecutor(Action::Open)))
        .then(
            CommandNode::literal("show")
                .execute(FactionExecutor(Action::ShowSelf))
                .then(str_node("faction").execute(FactionExecutor(Action::ShowNamed))),
        )
        .then(CommandNode::literal("i").execute(FactionExecutor(Action::ShowSelf)))
        .then(CommandNode::literal("info").execute(FactionExecutor(Action::ShowSelf)))
        .then(CommandNode::literal("who").execute(FactionExecutor(Action::ShowSelf)))
        .then(CommandNode::literal("list").execute(FactionExecutor(Action::List)))
        .then(
            CommandNode::literal("top")
                .execute(FactionExecutor(Action::TopDefault))
                .then(str_node("metric").execute(FactionExecutor(Action::TopMetric))),
        )
        .then(CommandNode::literal("deposit").then(str_node("amount").execute(FactionExecutor(Action::Deposit))))
        .then(CommandNode::literal("d").then(str_node("amount").execute(FactionExecutor(Action::Deposit))))
        .then(CommandNode::literal("withdraw").then(str_node("amount").execute(FactionExecutor(Action::Withdraw))))
        .then(CommandNode::literal("w").then(str_node("amount").execute(FactionExecutor(Action::Withdraw))))
        .then(CommandNode::literal("ally").then(str_node("faction").execute(FactionExecutor(Action::Ally))))
        .then(CommandNode::literal("unally").then(str_node("faction").execute(FactionExecutor(Action::Unally))))
        // ---- Admin subcommands ----
        .then(
            CommandNode::literal("setdtr").then(
                str_node("faction").then(
                    CommandNode::argument("value", &ArgumentType::Double((None, None)))
                        .execute(FactionExecutor(Action::AdminSetDtr)),
                ),
            ),
        )
        .then(
            CommandNode::literal("setpoints").then(
                str_node("faction").then(int_node("value").execute(FactionExecutor(Action::AdminSetPoints))),
            ),
        )
        .then(
            CommandNode::literal("setbalance").then(
                str_node("faction").then(int_node("value").execute(FactionExecutor(Action::AdminSetBalance))),
            ),
        )
        .then(
            CommandNode::literal("setlives").then(
                str_node("faction").then(int_node("value").execute(FactionExecutor(Action::AdminSetLives))),
            ),
        )
        .then(CommandNode::literal("forcekick").then(str_node("player").execute(FactionExecutor(Action::AdminForceKick))))
        .then(CommandNode::literal("forceleader").then(str_node("player").execute(FactionExecutor(Action::AdminForceLeader))))
        .then(CommandNode::literal("freeze").then(str_node("faction").execute(FactionExecutor(Action::AdminFreeze))))
        .then(CommandNode::literal("thaw").then(str_node("faction").execute(FactionExecutor(Action::AdminThaw))))
        .then(CommandNode::literal("save").execute(FactionExecutor(Action::AdminSave)));

    context.register_command(root, "lazarus:command.faction");
    Ok(())
}

fn run(action: Action, sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let Some(player) = sender.as_player() else {
        reply(sender, "&cThis command is for players only.");
        return;
    };
    let uuid = core::uuid_of(&player);
    let name = core::name_of(&player);

    match action {
        Action::Create => create(sender, &player, &uuid, &name, args),
        Action::Disband => disband(sender, server, &uuid),
        Action::Invite => invite(sender, server, &uuid, args),
        Action::Join => join(sender, server, &player, &uuid, &name, args),
        Action::Leave => leave(sender, server, &player, &uuid, &name),
        Action::Kick => kick(sender, server, &uuid, args),
        Action::Promote => change_role(sender, server, &uuid, args, true),
        Action::Demote => change_role(sender, server, &uuid, args, false),
        Action::Leader => transfer_leader(sender, server, &uuid, args),
        Action::Rename => rename(sender, &uuid, args),
        Action::Chat => toggle_chat(sender, &uuid),
        Action::Claim => claim(sender, &player, &uuid),
        Action::Unclaim => unclaim(sender, &player, &uuid),
        Action::UnclaimAll => unclaim_all(sender, &uuid),
        Action::Map => map(sender, &player, &uuid),
        Action::Territory => territory_info(sender, &player, &uuid, None),
        Action::TerritoryCoords => {
            let x = int_arg(args, "x").unwrap_or(0) as i32;
            let z = int_arg(args, "z").unwrap_or(0) as i32;
            territory_info(sender, &player, &uuid, Some((x, z)))
        }
        Action::Help => {}
        Action::HelpPage => {}
        Action::SetHome => set_home(sender, &player, &uuid),
        Action::Home => home(sender, &player, &uuid),
        Action::Stuck => stuck(sender, &player, &uuid),
        Action::Open => toggle_open(sender, server, &uuid),
        Action::ShowSelf => show(sender, server, &uuid, None),
        Action::ShowNamed => show(sender, server, &uuid, string_arg(args, "faction")),
        Action::List => list(sender),
        Action::TopDefault => top(sender, "points"),
        Action::TopMetric => top(sender, &string_arg(args, "metric").unwrap_or_else(|| "points".into())),
        Action::Deposit => transfer_balance(sender, &uuid, args, true),
        Action::Withdraw => transfer_balance(sender, &uuid, args, false),
        Action::Ally => ally(sender, server, &uuid, args, true),
        Action::Unally => ally(sender, server, &uuid, args, false),
        Action::AdminSetDtr => admin_set_dtr(sender, server, args),
        Action::AdminSetPoints => admin_set_int(sender, server, args, AdminField::Points),
        Action::AdminSetBalance => admin_set_int(sender, server, args, AdminField::Balance),
        Action::AdminSetLives => admin_set_int(sender, server, args, AdminField::Lives),
        Action::AdminForceKick => admin_force_kick(sender, server, args),
        Action::AdminForceLeader => admin_force_leader(sender, server, args),
        Action::AdminFreeze => admin_freeze(sender, server, args, true),
        Action::AdminThaw => admin_freeze(sender, server, args, false),
        Action::AdminSave => admin_save(sender),
    }
}

// ---------------------------------------------------------------------------
// Individual subcommands
// ---------------------------------------------------------------------------

fn create(sender: &CommandSender, _player: &Player, uuid: &str, name: &str, args: &ConsumedArgs) {
    let Some(faction_name) = string_arg(args, "name") else {
        reply(sender, &state().read().unwrap().lang.usage_create);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let config = guard.config.clone();
        let lang = guard.lang.clone();

        if guard.factions.by_member(uuid).is_some() {
            reply(sender, &lang.faction_already_in_self);
            return;
        }

        if faction_name.chars().count() < config.factions.name_min_length {
            reply(sender, util::replace(&lang.faction_name_too_short, "<min>", &config.factions.name_min_length.to_string()));
            return;
        }
        if faction_name.chars().count() > config.factions.name_max_length {
            reply(sender, util::replace(&lang.faction_name_too_long, "<max>", &config.factions.name_max_length.to_string()));
            return;
        }
        if config.factions.disallowed_names.iter().any(|n| n.eq_ignore_ascii_case(&faction_name)) {
            reply(sender, &lang.faction_blocked_name);
            return;
        }
        if guard.factions.name_taken(&faction_name) {
            reply(sender, util::replace(&lang.faction_already_exists, "<name>", &faction_name));
            return;
        }

        let id = guard.factions.create(faction_name.clone(), &config);
        guard.factions.add_member(&id, member(uuid.to_string(), name.to_string(), Role::Leader));

        messages = util::replace(&lang.faction_created, "<name>", &faction_name)
            .replace("<player>", name);
    }

    reply(sender, messages);
}

fn disband(sender: &CommandSender, server: &Server, uuid: &str) {
    let messages: Option<String>;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let config = guard.config.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();

        if !is_leader(&faction, uuid) {
            reply(sender, &lang.faction_leader_only);
            return;
        }
        if config.factions.disband_while_frozen && guard.timers.is_frozen(&faction_id) {
            reply(sender, &lang.faction_cannot_disband_frozen);
            return;
        }

        guard.claims.remove_all(&faction_id);
        guard.factions.disband(&faction_id);

        let message = util::replace(
            &util::replace(&lang.faction_disbanded, "<name>", &faction.name),
            "<player>",
            &sender.get_name(),
        );
        // `PlayerFaction#sendMessage` reaches every member, the commanding leader
        // included, so there is no separate reply here (that caused a double send).
        for member in faction.members.values() {
            if let Some(uuid) = pumpkin_plugin_api::uuid::parse(&member.uuid)
                && let Some(player) = server.get_player_by_uuid(uuid)
            {
                core::send(&player, &message);
            }
        }
        messages = None;
    }
    if let Some(message) = messages {
        reply(sender, message);
    }
}

fn invite(sender: &CommandSender, server: &Server, uuid: &str, args: &ConsumedArgs) {
    let Some(target_name) = string_arg(args, "player") else {
        reply(sender, &state().read().unwrap().lang.usage_invite);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let config = guard.config.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };

        let Some(target) = server.get_player_by_name(&target_name) else {
            reply(sender, util::replace(&lang.player_not_online, "<player>", &target_name));
            return;
        };
        let target_uuid = core::uuid_of(&target);

        if guard.factions.by_member(&target_uuid).is_some() {
            reply(sender, util::replace(&lang.faction_already_in_others, "<player>", &target_name));
            return;
        }

        let Some(faction) = guard.factions.get(&faction_id) else {
            return;
        };

        if faction.invitations.contains(&target_uuid) {
            reply(sender, util::replace(&lang.faction_invite_already, "<player>", &target_name));
            return;
        }
        if faction.members.len() >= config.factions.faction_limit {
            reply(sender, &lang.faction_full);
            return;
        }

        let faction_name = faction.name.clone();
        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            faction.invitations.push(target_uuid.clone());
        }

        let to_target = util::replace(&lang.faction_invited, "<name>", &faction_name);
        let to_sender = util::replace(&lang.faction_invite_others, "<player>", &target_name);

        core::send(&target, &to_target);
        messages = to_sender;
    }
    reply(sender, messages);
}

fn join(sender: &CommandSender, server: &Server, player: &Player, uuid: &str, name: &str, args: &ConsumedArgs) {
    let Some(faction_name) = string_arg(args, "name") else {
        reply(sender, &state().read().unwrap().lang.usage_join);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let config = guard.config.clone();

        if guard.factions.by_member(uuid).is_some() {
            reply(sender, &lang.faction_already_in_self);
            return;
        }

        let Some(faction_id) = guard.factions.by_name(&faction_name).filter(|f| f.is_player()).map(|f| f.id.clone()) else {
            reply(sender, util::replace(&lang.faction_doesnt_exist, "<argument>", &faction_name));
            return;
        };

        let invited = guard.factions.get(&faction_id).is_some_and(|f| f.invitations.contains(&uuid.to_string()));
        let open = guard.factions.get(&faction_id).is_some_and(|f| f.open);
        if !invited && !open {
            reply(sender, &lang.faction_no_invitation);
            return;
        }

        if config.factions.join_while_frozen && guard.timers.is_frozen(&faction_id) {
            reply(sender, &lang.faction_cannot_join_frozen);
            return;
        }

        if let Some(faction) = guard.factions.get(&faction_id) {
            if faction.members.len() >= config.factions.faction_limit {
                reply(sender, &lang.faction_full);
                return;
            }
        }

        guard.factions.add_member(&faction_id, member(uuid.to_string(), name.to_string(), Role::Member));
        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            faction.invitations.retain(|id| id != uuid);
        }
        guard
            .chunk_cache
            .insert(uuid.to_string(), ChunkState::default());

        let faction = guard.factions.get(&faction_id).cloned().unwrap();
        let broadcast = util::replace(&lang.faction_member_joined, "<player>", name);
        for member in faction.members.values() {
            if member.uuid == uuid {
                continue;
            }
            if let Some(id) = pumpkin_plugin_api::uuid::parse(&member.uuid)
                && let Some(online) = server.get_player_by_uuid(id)
            {
                core::send(&online, &broadcast);
            }
        }
        messages = util::replace(&lang.faction_joined, "<player>", name);
        let _ = player;
    }
    reply(sender, messages);
}

fn leave(sender: &CommandSender, server: &Server, player: &Player, uuid: &str, name: &str) {
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let config = guard.config.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();

        if is_leader(&faction, uuid) && faction.members.len() > 1 {
            reply(sender, &lang.faction_leader_only);
            return;
        }
        if !config.factions.leave_while_frozen && guard.timers.is_frozen(&faction_id) {
            reply(sender, &lang.faction_cannot_leave_frozen);
            return;
        }

        guard.factions.remove_member(&faction_id, uuid);
        if faction.members.len() <= 1 {
            guard.claims.remove_all(&faction_id);
            guard.factions.disband(&faction_id);
        }

        let broadcast = util::replace(&lang.faction_member_left, "<player>", name);
        for member in faction.members.values() {
            if member.uuid == uuid {
                continue;
            }
            if let Some(id) = pumpkin_plugin_api::uuid::parse(&member.uuid)
                && let Some(online) = server.get_player_by_uuid(id)
            {
                core::send(&online, &broadcast);
            }
        }
        messages = util::replace(&lang.faction_left, "<name>", &faction.name);
        let _ = player;
    }
    reply(sender, messages);
}

fn kick(sender: &CommandSender, server: &Server, uuid: &str, args: &ConsumedArgs) {
    let Some(target_name) = string_arg(args, "player") else {
        reply(sender, &state().read().unwrap().lang.usage_kick);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();

        let Some(sender_role) = faction.member(uuid).map(|m| m.role) else {
            return;
        };
        if !sender_role.is_at_least(Role::Captain) {
            reply(sender, &lang.faction_leader_only);
            return;
        }

        let Some(target) = server.get_player_by_name(&target_name) else {
            reply(sender, util::replace(&lang.player_not_online, "<player>", &target_name));
            return;
        };
        let target_uuid = core::uuid_of(&target);

        let Some(target_role) = faction.member(&target_uuid).map(|m| m.role) else {
            reply(sender, util::replace(&lang.faction_not_in_others, "<player>", &target_name));
            return;
        };
        if target_role.is_at_least(sender_role) {
            reply(sender, &lang.faction_leader_only);
            return;
        }
        if target_uuid == uuid {
            return;
        }

        guard.factions.remove_member(&faction_id, &target_uuid);
        core::send(&target, &util::replace(&lang.faction_you_were_kicked, "<name>", &faction.name));

        messages = util::replace(&lang.faction_kicked, "<player>", &target_name);
    }
    reply(sender, messages);
}

fn change_role(sender: &CommandSender, server: &Server, uuid: &str, args: &ConsumedArgs, promote: bool) {
    let Some(target_name) = string_arg(args, "player") else {
        let usage = if promote {
            state().read().unwrap().lang.usage_promote.clone()
        } else {
            state().read().unwrap().lang.usage_demote.clone()
        };
        reply(sender, usage);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();

        let Some(sender_role) = faction.member(uuid).map(|m| m.role) else {
            return;
        };
        if !sender_role.is_at_least(Role::CoLeader) {
            reply(sender, &lang.faction_leader_only);
            return;
        }

        let Some(target) = server.get_player_by_name(&target_name) else {
            reply(sender, util::replace(&lang.player_not_online, "<player>", &target_name));
            return;
        };
        let target_uuid = core::uuid_of(&target);

        let Some(target_role) = faction.member(&target_uuid).map(|m| m.role) else {
            reply(sender, util::replace(&lang.faction_not_in_others, "<player>", &target_name));
            return;
        };

        let new_role = if promote { target_role.promote() } else { target_role.demote() };
        let Some(new_role) = new_role else {
            return;
        };

        if let Some(faction) = guard.factions.get_mut(&faction_id)
            && let Some(member) = faction.member_mut(&target_uuid)
        {
            member.role = new_role;
        }

        let template = if promote { &lang.faction_promoted } else { &lang.faction_demoted };
        messages = util::replace(
            &util::replace(template, "<player>", &target_name),
            "<role>",
            new_role.name(),
        );
    }
    reply(sender, messages);
}

fn transfer_leader(sender: &CommandSender, server: &Server, uuid: &str, args: &ConsumedArgs) {
    let Some(target_name) = string_arg(args, "player") else {
        reply(sender, &state().read().unwrap().lang.usage_leader);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();
        if !is_leader(&faction, uuid) {
            reply(sender, &lang.faction_leader_only);
            return;
        }

        let Some(target) = server.get_player_by_name(&target_name) else {
            reply(sender, util::replace(&lang.player_not_online, "<player>", &target_name));
            return;
        };
        let target_uuid = core::uuid_of(&target);
        if !faction.members.contains_key(&target_uuid) {
            reply(sender, util::replace(&lang.faction_not_in_others, "<player>", &target_name));
            return;
        }

        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            if let Some(old_leader) = faction.member_mut(uuid) {
                old_leader.role = Role::CoLeader;
            }
            if let Some(new_leader) = faction.member_mut(&target_uuid) {
                new_leader.role = Role::Leader;
            }
        }

        messages = util::replace(
            &util::replace(&lang.faction_leader_changed, "<player>", &sender.get_name()),
            "<target>",
            &target_name,
        );
    }
    reply(sender, messages);
}

fn rename(sender: &CommandSender, uuid: &str, args: &ConsumedArgs) {
    let Some(new_name) = string_arg(args, "name") else {
        reply(sender, &state().read().unwrap().lang.usage_rename);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let config = guard.config.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();
        if !is_leader(&faction, uuid) {
            reply(sender, &lang.faction_leader_only);
            return;
        }
        if guard.factions.name_taken(&new_name) {
            reply(sender, util::replace(&lang.faction_already_exists, "<name>", &new_name));
            return;
        }
        if new_name.chars().count() < config.factions.name_min_length
            || new_name.chars().count() > config.factions.name_max_length
        {
            reply(sender, &lang.faction_blocked_name);
            return;
        }
        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            faction.name = new_name.clone();
        }
        messages = util::replace(
            &util::replace(&lang.faction_renamed, "<name>", &faction.name),
            "<newName>",
            &new_name,
        );
    }
    reply(sender, messages);
}

fn toggle_chat(sender: &CommandSender, uuid: &str) {
    let mut guard = state().write().unwrap();
    let lang = guard.lang.clone();
    let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
        reply(sender, &lang.faction_not_in);
        return;
    };
    let mut new_type = crate::factions::ChatType::Public;
    if let Some(faction) = guard.factions.get_mut(&faction_id)
        && let Some(member) = faction.member_mut(uuid)
    {
        member.chat_type = match member.chat_type {
            crate::factions::ChatType::Public => crate::factions::ChatType::Faction,
            crate::factions::ChatType::Faction => crate::factions::ChatType::Ally,
            crate::factions::ChatType::Ally => crate::factions::ChatType::Public,
        };
        new_type = member.chat_type;
    }
    let status = match new_type {
        crate::factions::ChatType::Public => "public",
        crate::factions::ChatType::Faction => "faction",
        crate::factions::ChatType::Ally => "ally",
    };
    reply(sender, util::replace(&lang.faction_chat_toggled, "<status>", status));
}

fn claim(sender: &CommandSender, player: &Player, uuid: &str) {
    let error = {
        let guard = state().read().unwrap();
        validate_claim(player, uuid, &guard.factions, &guard.claims, &guard.config, &guard.lang)
    };

    if let Err(message) = error {
        reply(sender, message);
        return;
    }

    // The original always (re)starts the selection process, it never toggles off.
    state().write().unwrap().selection.start(player);
    let message = state().read().unwrap().lang.faction_claim_wand_received.clone();
    reply(sender, message);
}

fn validate_claim(
    player: &Player,
    uuid: &str,
    factions: &FactionsManager,
    claims: &ClaimManager,
    config: &Config,
    lang: &Lang,
) -> Result<(), String> {
    let Some(faction_id) = factions.by_member(uuid).map(|f| f.id.clone()) else {
        return Err(lang.faction_not_in.clone());
    };
    let faction = factions.get(&faction_id).unwrap();

    if faction
        .member(uuid)
        .map(|m| m.role)
        .is_none_or(|role| !role.is_at_least(Role::CoLeader))
    {
        return Err(lang.faction_leader_only.clone());
    }

    let max_claims = config
        .factions
        .max_claims
        .min(config.factions.claims_per_player * faction.members.len());
    if claims.claim_count(&faction_id) >= max_claims {
        return Err(lang.faction_claim_limit.clone());
    }

    if core::dimension_of(&core::world_name(player)) != core::Dimension::Overworld {
        return Err(lang.faction_claim_only_overworld.clone());
    }

    let location = core::location(player);
    let warzone = config.warzone_radius.overworld;
    if location.block_x().abs() <= warzone && location.block_z().abs() <= warzone {
        return Err(util::replace(
            &lang.faction_claim_spawn_too_close,
            "<amount>",
            &warzone.to_string(),
        ));
    }

    if (0..36u8).all(|slot| player.get_inventory_item(slot).is_some()) {
        return Err(lang.faction_claim_inventory_full.clone());
    }

    Ok(())
}

fn unclaim(sender: &CommandSender, player: &Player, uuid: &str) {
    let loc = core::location(player);
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let owned = guard
            .claims
            .faction_at(&loc.world, loc.block_x(), loc.block_z())
            .map(|owner| owner == &faction_id)
            .unwrap_or(false);
        if !owned {
            reply(sender, &lang.faction_claim_not_owned);
            return;
        }
        guard
            .claims
            .remove_claim_at(&faction_id, &loc.world, loc.block_x(), loc.block_z());
        messages = util::replace(
            &util::replace(&lang.faction_claim_removed, "<player>", &sender.get_name()),
            "<location>",
            &format!("{}, {}", loc.block_x(), loc.block_z()),
        );
    }
    reply(sender, messages);
}

fn unclaim_all(sender: &CommandSender, uuid: &str) {
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();
        if !is_leader(&faction, uuid) {
            reply(sender, &lang.faction_leader_only);
            return;
        }
        guard.claims.remove_all(&faction_id);
        messages = "&aAll faction claims removed.".to_string();
    }
    reply(sender, messages);
}

fn map(sender: &CommandSender, player: &Player, uuid: &str) {
    let world = core::world_name(player);
    let message;
    {
        let mut guard = state().write().unwrap();
        let AppState { glass, claims, config, .. } = &mut *guard;

        if glass.is_map_user(uuid) {
            glass.clear_type(player, GlassType::ClaimMap);
            glass.remove_map_user(uuid);
            message = "&eAll claim pillars are now &chidden&e.".to_string();
        } else {
            // Force a regeneration on the next map update.
            glass.set_map_user(uuid, &world, i32::MIN, i32::MIN);
            crate::glass::update_map(glass, player, claims, config);
            message = if glass.is_map_user(uuid) {
                "&aClaim map enabled.".to_string()
            } else {
                "&cThere are no claims nearby.".to_string()
            };
        }
    }
    reply(sender, message);
}

/// Diagnostic helper (not part of the original): prints the territory square with
/// the warzone/safezone bounds centered at 0,0.
fn territory_info(
    sender: &CommandSender,
    player: &Player,
    uuid: &str,
    coords: Option<(i32, i32)>,
) {
    let location = core::location(player);
    let world = location.world.clone();
    let (bx, bz) = coords.unwrap_or((location.block_x(), location.block_z()));

    let (war_radius, safe_radius) = {
        let guard = state().read().unwrap();
        let war = match core::dimension_of(&world) {
            core::Dimension::Overworld => guard.config.warzone_radius.overworld,
            core::Dimension::Nether => guard.config.warzone_radius.nether,
            core::Dimension::End => guard.config.warzone_radius.end,
        };
        (war, guard.config.spawn.safezone_radius)
    };

    let r = war_radius;
    let s = safe_radius;
    let dist = bx.abs().max(bz.abs());

    let territory = core::territory_at(&world, bx, bz);
    let viewer = core::faction_id_of(uuid);
    let display = core::territory_display(&territory, viewer.as_deref());

    let reason = if dist <= s {
        "Safezone (dist <= safezone)"
    } else if dist <= r {
        "Warzone (dist <= warzone)"
    } else {
        "Wilderness (dist > warzone)"
    };

    let sep = format!("&7&m{}", " ".repeat(80));
    reply(sender, &sep);
    reply(sender, "&6&lWarzone square (centro 0,0)");
    reply(sender, format!(" &eWorld: &f{world}"));
    reply(sender, format!(" &eSafezone: &fX[-{s}, {s}] &7Z[&f-{s}, {s}&7] &8(|x|,|z| <= {s})"));
    reply(sender, format!(" &eWarzone: &fX[-{r}, {r}] &7Z[&f-{r}, {r}&7] &8(|x|,|z| <= {r})"));
    reply(
        sender,
        format!(
            " &eCorners: &f(-{r},-{r}) (-{r},{r}) ({r},-{r}) ({r},{r})"
        ),
    );
    reply(sender, format!(" &eLado: &f{} &ebloques por eje (inclusivo)", r * 2 + 1));
    reply(sender, " &7Protección &finfinita en Y&7 (arriba/abajo).");
    reply(sender, &sep);

    reply(sender, format!(" &eQuery: &f{bx}, {bz}"));
    reply(sender, format!(" &eDist: &fmax(|x|,|z|) = {dist}"));
    reply(sender, format!(" &eCurrent: {display}"));
    reply(sender, format!(" &eReason: &f{reason}"));
    reply(sender, &sep);

    // Corner verification: every corner must be Warzone, and one block outside must
    // be Wilderness — proves the square on all four sides.
    reply(sender, "&6&lEsquinas del cuadrado (deben ser Warzone):");
    for (cx, cz) in [(-r, -r), (-r, r), (r, -r), (r, r)] {
        let t = core::territory_at(&world, cx, cz);
        let ok = matches!(t, core::Territory::Warzone);
        let viewer = core::faction_id_of(uuid);
        let d = core::territory_display(&t, viewer.as_deref());
        reply(sender, format!(" &e({cx}, {cz}) -> {d}  {}", if ok { "&aOK" } else { "&cFALLO" }));
    }

    reply(sender, "&6&l1 bloque fuera (deben ser Wilderness):");
    for (cx, cz) in [(r + 1, 0), (-(r + 1), 0), (0, r + 1), (0, -(r + 1))] {
        let t = core::territory_at(&world, cx, cz);
        let ok = matches!(t, core::Territory::Wilderness);
        let viewer = core::faction_id_of(uuid);
        let d = core::territory_display(&t, viewer.as_deref());
        reply(sender, format!(" &e({cx}, {cz}) -> {d}  {}", if ok { "&aOK" } else { "&cFALLO" }));
    }
    reply(sender, &sep);
}

fn set_home(sender: &CommandSender, player: &Player, uuid: &str) {
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();
        if faction.member(uuid).map(|m| m.role).is_none_or(|role| !role.is_at_least(Role::CoLeader)) {
            reply(sender, &lang.faction_leader_only);
            return;
        }
        let home = core::location(player);
        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            faction.home = Some(home);
        }
        messages = lang.faction_home_set.clone();
    }
    reply(sender, messages);
}

fn home(sender: &CommandSender, player: &Player, uuid: &str) {
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let Some(home) = guard.factions.get(&faction_id).and_then(|f| f.home.clone()) else {
            reply(sender, &lang.faction_home_not_set);
            return;
        };

        let warmup = match core::dimension_of(&home.world) {
            core::Dimension::Overworld => guard.config.home_warmup.overworld,
            core::Dimension::Nether => guard.config.home_warmup.nether,
            core::Dimension::End => guard.config.home_warmup.end,
        };

        let origin = core::location(player);
        let expiry = crate::util::now_millis() + (warmup as i64 * 1000);
        guard.timers.set_pending(
            uuid,
            PendingAction {
                timer_type: TimerType::Home,
                expiry,
                origin_world: origin.world,
                origin_x: origin.x,
                origin_y: origin.y,
                origin_z: origin.z,
                target: Some(home),
            },
        );

        messages = util::replace(&lang.faction_home_teleport, "<seconds>", &warmup.to_string());
    }
    reply(sender, messages);
}

fn stuck(sender: &CommandSender, player: &Player, uuid: &str) {
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let warmup = guard.config.timers.stuck_warmup_seconds;
        let origin = core::location(player);

        guard.timers.set_pending(
            uuid,
            PendingAction {
                timer_type: TimerType::Stuck,
                expiry: crate::util::now_millis() + (warmup as i64 * 1000),
                origin_world: origin.world.clone(),
                origin_x: origin.x,
                origin_y: origin.y,
                origin_z: origin.z,
                target: None,
            },
        );

        messages = util::replace(
            &lang.stuck_warmup,
            "<time>",
            &util::format_duration(warmup as i64 * 1000),
        );
    }
    reply(sender, messages);
}

fn toggle_open(sender: &CommandSender, server: &Server, uuid: &str) {
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();
        if !is_leader(&faction, uuid) {
            reply(sender, &lang.faction_leader_only);
            return;
        }

        let new_value = !faction.open;
        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            faction.open = new_value;
        }
        messages = if new_value {
            util::replace(&lang.faction_opened, "<name>", &faction.name)
        } else {
            util::replace(&lang.faction_closed, "<name>", &faction.name)
        };
        let _ = server;
    }
    reply(sender, messages);
}

fn show(sender: &CommandSender, server: &Server, uuid: &str, name: Option<String>) {
    let guard = state().read().unwrap();
    let lang = guard.lang.clone();

    let faction = match &name {
        Some(name) => guard.factions.by_name(name).cloned(),
        None => guard.factions.by_member(uuid).cloned(),
    };

    let Some(faction) = faction else {
        reply(sender, &lang.faction_not_in);
        return;
    };

    // Relation colour for the faction name (as seen by the sender).
    let display = {
        let viewer = guard.factions.by_member(uuid);
        let color = match viewer {
            Some(v) if v.id == faction.id => &guard.config.relation_colors.teammate,
            Some(v) if v.is_ally(&faction.id) => &guard.config.relation_colors.ally,
            _ => &guard.config.relation_colors.enemy,
        };
        format!("{color}{}", faction.name)
    };

    let online = faction
        .members
        .values()
        .filter(|m| {
            pumpkin_plugin_api::uuid::parse(&m.uuid)
                .and_then(|id| server.get_player_by_uuid(id))
                .is_some()
        })
        .count();

    let role_names = |role: Role| -> String {
        let names: Vec<String> = faction
            .members
            .values()
            .filter(|m| m.role == role)
            .map(|m| format!("{}{}", guard.config.relation_colors.teammate, m.name))
            .collect();
        if names.is_empty() { lang.none.clone() } else { names.join("&7, ") }
    };

    let allies: Vec<String> = faction
        .allies
        .iter()
        .filter_map(|id| guard.factions.get(id).map(|f| f.name.clone()))
        .collect();
    let allies = if allies.is_empty() { lang.none.clone() } else { allies.join("&7, &f") };

    let max_dtr = faction.max_dtr(&guard.config);
    let dtr_color = if faction.dtr <= 0.0 { "&c" } else { "&a" };
    let raidable = if faction.is_raidable() { lang.yes.clone() } else { lang.no.clone() };

    let sep = format!("&7&m{}", " ".repeat(80));

    reply(sender, &sep);
    reply(sender, format!(" {display} &7[{online}/{}] &e- &eHome: &7{}", faction.members.len(), faction.home_string()));
    reply(sender, format!(" &eAnnouncement: &7{}", faction.announcement.clone().unwrap_or_else(|| lang.none.clone())));
    reply(sender, format!(" &eAuto revive: &7{}", lang.no));
    reply(sender, format!(" &eFaction lives: &7{}", faction.lives));
    reply(sender, format!(" &eTotal Kills: &7{}", faction.kills));
    reply(sender, format!(" &ePoints: &7{}", faction.points));
    reply(sender, format!(" &eKoTH captures: &7{}", faction.koths_capped));
    reply(sender, format!(" &eBalance: &7${}", faction.balance));
    reply(sender, format!(" &eLeader: {}", role_names(Role::Leader)));
    reply(sender, format!(" &eCo-leaders: {}", role_names(Role::CoLeader)));
    reply(sender, format!(" &eCaptains: {}", role_names(Role::Captain)));
    reply(sender, format!(" &eMembers: {}", role_names(Role::Member)));
    reply(sender, format!(" &eAllies: {allies}"));
    reply(sender, format!(" &eRegenerating: &7{}", lang.none));
    reply(sender, format!(" &eDTR: {dtr_color}{:.2}&7/&a{max_dtr:.2} &eRaidable: {raidable}", faction.dtr));
    reply(sender, &sep);
}

fn list(sender: &CommandSender) {
    let guard = state().read().unwrap();
    reply(sender, "&3=== &bFactions&3 ===");
    for faction in guard.factions.player_factions() {
        reply(
            sender,
            format!("&f{} &7- &e{} members &7- &c{:.2} DTR", faction.name, faction.members.len(), faction.dtr),
        );
    }
}

fn top(sender: &CommandSender, metric: &str) {
    let guard = state().read().unwrap();
    let mut factions: Vec<&Faction> = guard.factions.player_factions().collect();
    factions.sort_by(|a, b| match metric.to_lowercase().as_str() {
        "kills" => b.kills.cmp(&a.kills),
        "balance" => b.balance.cmp(&a.balance),
        "koths" => b.koths_capped.cmp(&a.koths_capped),
        _ => b.points.cmp(&a.points),
    });

    reply(sender, format!("&3=== &bTop factions ({metric})&3 ==="));
    for (index, faction) in factions.into_iter().take(10).enumerate() {
        let value = match metric.to_lowercase().as_str() {
            "kills" => faction.kills as i64,
            "balance" => faction.balance,
            "koths" => faction.koths_capped as i64,
            _ => faction.points as i64,
        };
        reply(sender, format!("&f{}. &e{} &7- &c{value}", index + 1, faction.name));
    }
}

fn transfer_balance(sender: &CommandSender, uuid: &str, args: &ConsumedArgs, deposit: bool) {
    let raw = string_arg(args, "amount").unwrap_or_default();

    let messages;
    {
        let mut guard = state().write().unwrap();

        // Destructure so the borrow checker sees disjoint fields (DerefMut would
        // otherwise borrow the whole guard through `guard.userdata`).
        let crate::state::AppState { userdata, factions, config, lang, .. } = &mut *guard;

        let max_balance = config.economy.max_balance;

        let Some(faction_id) = factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = factions.get(&faction_id).cloned().unwrap();

        if !deposit
            && faction.member(uuid).map(|m| m.role).is_none_or(|role| !role.is_at_least(Role::Captain))
        {
            reply(sender, &lang.faction_leader_only);
            return;
        }

        let Some(user) = userdata.get_mut(uuid) else {
            return;
        };

        // Support `/f deposit all` / `/f withdraw all` like the original.
        let amount = if raw.eq_ignore_ascii_case("all") {
            if deposit { user.balance } else { faction.balance }
        } else {
            raw.parse::<i64>().unwrap_or(0)
        };

        if amount <= 0 {
            let usage = if deposit { lang.usage_deposit.clone() } else { lang.usage_withdraw.clone() };
            reply(sender, usage);
            return;
        }

        if deposit {
            if user.balance < amount {
                reply(sender, &lang.faction_not_enough_money);
                return;
            }
            user.balance -= amount;
            if let Some(faction) = factions.get_mut(&faction_id) {
                faction.balance = (faction.balance + amount).min(max_balance);
            }
            messages = util::replace(
                &util::replace(&lang.faction_deposit_deposited, "<player>", &sender.get_name()),
                "<amount>",
                &amount.to_string(),
            );
        } else {
            if faction.balance < amount {
                reply(
                    sender,
                    util::replace(&lang.faction_withdraw_not_enough, "<amount>", &faction.balance.to_string()),
                );
                return;
            }
            if let Some(faction) = factions.get_mut(&faction_id) {
                faction.balance -= amount;
            }
            user.balance = (user.balance + amount).min(max_balance);
            messages = util::replace(
                &util::replace(&lang.faction_withdraw_withdrawn, "<player>", &sender.get_name()),
                "<amount>",
                &amount.to_string(),
            );
        }
    }
    reply(sender, messages);
}

fn ally(sender: &CommandSender, server: &Server, uuid: &str, args: &ConsumedArgs, add: bool) {
    let Some(name) = string_arg(args, "faction") else {
        let usage = if add {
            state().read().unwrap().lang.usage_ally.clone()
        } else {
            state().read().unwrap().lang.usage_unally.clone()
        };
        reply(sender, usage);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let max_allies = guard.config.factions.max_allies;

        let Some(my_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let Some(other_id) = guard.factions.by_name(&name).filter(|f| f.is_player()).map(|f| f.id.clone()) else {
            reply(sender, util::replace(&lang.faction_doesnt_exist, "<argument>", &name));
            return;
        };
        if my_id == other_id {
            return;
        }

        if add {
            let ally_count = guard.factions.get(&my_id).map(|f| f.allies.len()).unwrap_or(0);
            if max_allies > 0 && ally_count >= max_allies {
                reply(sender, &lang.faction_ally_limit);
                return;
            }
            if let Some(faction) = guard.factions.get_mut(&my_id)
                && !faction.allies.contains(&other_id)
            {
                faction.allies.push(other_id.clone());
            }
            if let Some(faction) = guard.factions.get_mut(&other_id)
                && !faction.allies.contains(&my_id)
            {
                faction.allies.push(my_id.clone());
            }
            messages = util::replace(&lang.faction_ally_added, "<faction>", &name);
        } else {
            if let Some(faction) = guard.factions.get_mut(&my_id) {
                faction.allies.retain(|id| id != &other_id);
            }
            if let Some(faction) = guard.factions.get_mut(&other_id) {
                faction.allies.retain(|id| id != &my_id);
            }
            messages = util::replace(&lang.faction_ally_removed, "<faction>", &name);
        }
        let _ = server;
    }
    reply(sender, messages);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_leader(faction: &Faction, uuid: &str) -> bool {
    faction.member(uuid).map(|m| m.role) == Some(Role::Leader)
}

fn is_connected(claims: &crate::factions::ClaimManager, faction_id: &str, world: &str, cx: i32, cz: i32) -> bool {
    let neighbors = [
        (cx + 1, cz),
        (cx - 1, cz),
        (cx, cz + 1),
        (cx, cz - 1),
        (cx + 1, cz + 1),
        (cx + 1, cz - 1),
        (cx - 1, cz + 1),
        (cx - 1, cz - 1),
    ];
    neighbors.iter().any(|(nx, nz)| {
        claims
            .faction_at(world, nx * 16, nz * 16)
            .map(|owner| owner == faction_id)
            .unwrap_or(false)
    })
}

// ---------------------------------------------------------------------------
// Admin subcommands
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum AdminField {
    Points,
    Balance,
    Lives,
}

fn require_admin(sender: &CommandSender, server: &Server) -> bool {
    if sender.is_console() || sender.has_permission(server, "lazarus:command.faction.admin") {
        return true;
    }
    reply(sender, "&cYou do not have permission.");
    false
}

fn admin_set_dtr(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_admin(sender, server) {
        return;
    }
    let Some(name) = string_arg(args, "faction") else {
        return;
    };
    let Some(value) = double_arg(args, "value") else {
        reply(sender, "&cInvalid number.");
        return;
    };

    let mut guard = state().write().unwrap();
    let config = guard.config.clone();
    let Some(id) = guard.factions.by_name(&name).map(|f| f.id.clone()) else {
        reply(sender, util::replace(&guard.lang.faction_doesnt_exist, "<argument>", &name));
        return;
    };
    if let Some(faction) = guard.factions.get_mut(&id) {
        faction.dtr = value.clamp(config.factions.min_dtr, config.factions.max_dtr);
    }
    reply(sender, format!("&eSet &c{name}&e's DTR to &c{value:.2}&e."));
}

fn admin_set_int(sender: &CommandSender, server: &Server, args: &ConsumedArgs, field: AdminField) {
    if !require_admin(sender, server) {
        return;
    }
    let Some(name) = string_arg(args, "faction") else {
        return;
    };
    let Some(value) = int_arg(args, "value") else {
        reply(sender, "&cInvalid number.");
        return;
    };

    let mut guard = state().write().unwrap();
    let Some(id) = guard.factions.by_name(&name).map(|f| f.id.clone()) else {
        reply(sender, util::replace(&guard.lang.faction_doesnt_exist, "<argument>", &name));
        return;
    };
    if let Some(faction) = guard.factions.get_mut(&id) {
        match field {
            AdminField::Points => faction.points = value as i32,
            AdminField::Balance => faction.balance = value,
            AdminField::Lives => faction.lives = value as i32,
        }
    }
    reply(sender, format!("&eUpdated &c{name} &e({value})."));
}

fn admin_force_kick(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_admin(sender, server) {
        return;
    }
    let Some(name) = string_arg(args, "player") else {
        return;
    };
    let Some(target) = server.get_player_by_name(&name) else {
        reply(sender, util::replace(&state().read().unwrap().lang.player_not_online, "<player>", &name));
        return;
    };
    let uuid = core::uuid_of(&target);

    let message;
    {
        let mut guard = state().write().unwrap();
        let Some(faction_id) = guard.factions.by_member(&uuid).map(|f| f.id.clone()) else {
            reply(sender, util::replace(&guard.lang.faction_not_in, "<player>", &name));
            return;
        };
        guard.factions.remove_member(&faction_id, &uuid);
        if guard.factions.get(&faction_id).is_some_and(|f| f.members.is_empty()) {
            guard.claims.remove_all(&faction_id);
            guard.factions.disband(&faction_id);
        }
        message = util::replace(&guard.lang.faction_admin_force_kicked, "<player>", &name);
    }
    reply(sender, message);
}

fn admin_force_leader(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_admin(sender, server) {
        return;
    }
    let Some(name) = string_arg(args, "player") else {
        return;
    };
    let Some(target) = server.get_player_by_name(&name) else {
        reply(sender, util::replace(&state().read().unwrap().lang.player_not_online, "<player>", &name));
        return;
    };
    let uuid = core::uuid_of(&target);

    let mut guard = state().write().unwrap();
    let Some(faction_id) = guard.factions.by_member(&uuid).map(|f| f.id.clone()) else {
        reply(sender, util::replace(&guard.lang.faction_not_in, "<player>", &name));
        return;
    };
    if let Some(faction) = guard.factions.get_mut(&faction_id) {
        for member in faction.members.values_mut() {
            if member.role == Role::Leader {
                member.role = Role::CoLeader;
            }
        }
        if let Some(member) = faction.members.get_mut(&uuid) {
            member.role = Role::Leader;
        }
    }
    reply(sender, format!("&eTransferred leadership to &c{name}&e."));
}

fn admin_freeze(sender: &CommandSender, server: &Server, args: &ConsumedArgs, freeze: bool) {
    if !require_admin(sender, server) {
        return;
    }
    let Some(name) = string_arg(args, "faction") else {
        return;
    };

    let mut guard = state().write().unwrap();
    let (Some(id), Some(faction_name), duration) = (
        guard.factions.by_name(&name).map(|f| f.id.clone()),
        guard.factions.by_name(&name).map(|f| f.name.clone()),
        guard.config.factions.freeze_duration_minutes,
    ) else {
        reply(sender, util::replace(&guard.lang.faction_doesnt_exist, "<argument>", &name));
        return;
    };

    if freeze {
        guard.timers.freeze_faction(&id, duration);
        reply(sender, format!("&c{faction_name}&e has been &cfrozen&e."));
    } else {
        guard.timers.unfreeze_faction(&id);
        reply(sender, format!("&c{faction_name}&e has been &athawed&e."));
    }
}

fn admin_save(sender: &CommandSender) {
    crate::state::save_all();
    reply(sender, "&aAll data saved.");
}

fn help(sender: &CommandSender, page: usize) {
    let line = "&7&m                                                                                ";

    let page_lines: [&[&str]; 4] = [
        &[
            " &6&lFaction Help &e(Page 1/4)",
            "",
            " &e/f join &f- &7Accept a join request from an faction.",
            " &e/f chat &f- &7Toggle faction chat only mode on or off.",
            " &e/f claim &f- &7Claim land in the Wilderness.",
            " &e/f create &f- &7Create a faction.",
            " &e/f disband &f- &7Disband your faction.",
            " &e/f home &f- &7Teleport to the faction home.",
            "",
            " &6You are currently on &ePage 1/4.",
            " &6To view other pages, use &e/f help <page>",
        ],
        &[
            " &6&lFaction Help &e(Page 2/4)",
            "",
            " &e/f invite &f- &7Invite a player to the faction.",
            " &e/f kick &f- &7Kick a player from the faction.",
            " &e/f leader &f- &7Sets the new leader for your faction.",
            " &e/f leave &f- &7Leave your current faction.",
            " &e/f list &f- &7See a list of all factions.",
            " &e/f map &f- &7View all claims around your location.",
            "",
            " &6You are currently on &ePage 2/4.",
            " &6To view other pages, use &e/f help <page>",
        ],
        &[
            " &6&lFaction Help &e(Page 3/4)",
            "",
            " &e/f rename &f- &7Change the name of your faction.",
            " &e/f promote &f- &7Promotes a player to a higher role.",
            " &e/f sethome &f- &7Sets the faction home location.",
            " &e/f show &f- &7Get details about a faction.",
            " &e/f unclaim &f- &7Unclaims land from your faction.",
            " &e/f deinvite &f- &7Revoke an invitation to a player.",
            "",
            " &6You are currently on &ePage 3/4.",
            " &6To view other pages, use &e/f help <page>",
        ],
        &[
            " &6&lFaction Help &e(Page 4/4)",
            "",
            " &e/f demote &f- &7Demotes a player to a lower role.",
            " &e/f deposit &f- &7Deposit money to your faction.",
            " &e/f withdraw &f- &7Withdraw money from your faction.",
            " &e/f stuck &f- &7Use it when stuck in enemy teritory.",
            "",
            " &6You are currently on &ePage 4/4.",
            " &6To view other pages, use &e/f help <page>.",
        ],
    ];

    let Some(lines) = page_lines.get(page.saturating_sub(1)) else {
        reply(sender, util::replace("&ePage &c<page> &enot found.", "<page>", &page.to_string()));
        return;
    };

    reply(sender, line);
    for entry in *lines {
        reply(sender, *entry);
    }
    reply(sender, line);
}
