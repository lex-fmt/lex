_lex() {
    local i cur prev opts cmd
    COMPREPLY=()
    if [[ "${BASH_VERSINFO[0]}" -ge 4 ]]; then
        cur="$2"
    else
        cur="${COMP_WORDS[COMP_CWORD]}"
    fi
    prev="$3"
    cmd=""
    opts=""

    for i in "${COMP_WORDS[@]:0:COMP_CWORD}"
    do
        case "${cmd},${i}" in
            ",$1")
                cmd="lex"
                ;;
            *)
                ;;
        esac
    done

    case "${cmd}" in
        lex)
            opts="-h -V --list-transforms --help --version"
            transforms="token-core-json token-core-simple token-core-pprint token-simple token-pprint token-line-json token-line-simple token-line-pprint ir-json ast-json ast-tag ast-treeviz"

            # Handle flags
            if [[ ${cur} == -* ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi

            # Count non-flag arguments
            local arg_count=0
            for word in "${COMP_WORDS[@]:1:COMP_CWORD-1}"; do
                if [[ ! ${word} == -* ]]; then
                    ((arg_count++))
                fi
            done

            # First positional argument: file path
            if [[ ${arg_count} -eq 0 ]]; then
                COMPREPLY=( $(compgen -f -- "${cur}") )
                return 0
            fi

            # Second positional argument: transform format
            if [[ ${arg_count} -eq 1 ]]; then
                COMPREPLY=( $(compgen -W "${transforms}" -- "${cur}") )
                return 0
            fi

            COMPREPLY=()
            return 0
            ;;
    esac
}

if [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 || "${BASH_VERSINFO[0]}" -gt 4 ]]; then
    complete -F _lex -o nosort -o bashdefault -o default -o filenames lex
else
    complete -F _lex -o bashdefault -o default -o filenames lex
fi
